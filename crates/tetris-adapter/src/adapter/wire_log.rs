//! Bounded, best-effort wire logging isolated from protocol delivery.

use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::adapter::protocol::{AckMessage, ErrorMessage, ObservationMessage, WelcomeMessage};

pub const WIRE_LOG_QUEUE_CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
pub(super) enum WireRecord {
    LineArc(Arc<str>),
    Welcome(WelcomeMessage),
    Ack(AckMessage),
    Error(ErrorMessage),
    ObservationArc(Arc<ObservationMessage>),
}

pub(super) fn try_log(log_tx: Option<&mpsc::Sender<WireRecord>>, record: WireRecord) {
    if let Some(tx) = log_tx {
        let _ = tx.try_send(record);
    }
}

pub(super) fn spawn_wire_logger(
    path: String,
    log_every_n: u64,
    log_max_lines: Option<u64>,
) -> mpsc::Sender<WireRecord> {
    let (tx, mut rx) = mpsc::channel::<WireRecord>(WIRE_LOG_QUEUE_CAPACITY);
    tokio::spawn(async move {
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(_) => return,
        };

        let mut buf = Vec::with_capacity(4096);
        let mut line_count = 0u64;
        let mut record_count = 0u64;
        let log_every_n = log_every_n.max(1);

        while let Some(record) = rx.recv().await {
            record_count = record_count.wrapping_add(1);
            if !record_count.is_multiple_of(log_every_n)
                || log_max_lines.is_some_and(|max| line_count >= max)
            {
                continue;
            }

            let write_result = match record {
                WireRecord::LineArc(line) => file.write_all(line.as_bytes()).await,
                WireRecord::Welcome(value) => write_json(&mut file, &mut buf, &value).await,
                WireRecord::Ack(value) => write_json(&mut file, &mut buf, &value).await,
                WireRecord::Error(value) => write_json(&mut file, &mut buf, &value).await,
                WireRecord::ObservationArc(value) => {
                    write_json(&mut file, &mut buf, value.as_ref()).await
                }
            };
            if write_result.is_err() || file.write_all(b"\n").await.is_err() {
                break;
            }
            line_count = line_count.wrapping_add(1);
        }

        let _ = file.flush().await;
    });
    tx
}

async fn write_json<T: serde::Serialize + ?Sized>(
    file: &mut tokio::fs::File,
    buf: &mut Vec<u8>,
    value: &T,
) -> std::io::Result<()> {
    buf.clear();
    serde_json::to_writer(&mut *buf, value).map_err(std::io::Error::other)?;
    file.write_all(buf).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_queue_drops_records_when_full() {
        let (tx, mut rx) = mpsc::channel(1);
        try_log(Some(&tx), WireRecord::LineArc(Arc::from("first")));
        try_log(Some(&tx), WireRecord::LineArc(Arc::from("second")));

        let WireRecord::LineArc(line) = rx.try_recv().unwrap() else {
            panic!("expected first wire record");
        };
        assert_eq!(line.as_ref(), "first");
        assert!(rx.try_recv().is_err());
    }
}
