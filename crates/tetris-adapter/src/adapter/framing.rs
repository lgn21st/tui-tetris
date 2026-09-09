//! TCP JSON-lines framing and per-client writer.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, watch};
use tokio::time::{Duration, MissedTickBehavior};

use crate::adapter::client_mailbox::ClientOutbound;
use crate::adapter::wire_log::{WireRecord, try_log as log_wire_record};

pub const MAX_INBOUND_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoundedLineRead {
    Eof,
    Line,
    TooLong,
}

pub(super) async fn read_bounded_line<R>(
    reader: &mut R,
    line: &mut Vec<u8>,
) -> std::io::Result<BoundedLineRead>
where
    R: AsyncBufRead + Unpin,
{
    line.clear();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if line.is_empty() {
                BoundedLineRead::Eof
            } else {
                BoundedLineRead::Line
            });
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let payload_len = newline.unwrap_or(available.len());
        if line.len().saturating_add(payload_len) > MAX_INBOUND_LINE_BYTES {
            return Ok(BoundedLineRead::TooLong);
        }

        let consumed = newline.map_or(available.len(), |index| index + 1);
        line.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(BoundedLineRead::Line);
        }
    }
}

pub(crate) fn encode_json_into_buf<T: serde::Serialize>(buf: &mut Vec<u8>, value: &T) -> bool {
    buf.clear();
    serde_json::to_writer(&mut *buf, value).is_ok()
}

async fn write_json_and_log<W, T, F>(
    writer: &mut BufWriter<W>,
    buf: &mut Vec<u8>,
    value: T,
    log_tx: Option<&mpsc::Sender<WireRecord>>,
    wrap: F,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: serde::Serialize,
    F: FnOnce(T) -> WireRecord,
{
    if !encode_json_into_buf(buf, &value) {
        return Ok(());
    }
    writer.write_all(buf).await?;
    log_wire_record(log_tx, wrap(value));
    Ok(())
}

pub(super) async fn run_client_writer<W>(
    mut writer: BufWriter<W>,
    mut reliable_rx: mpsc::Receiver<ClientOutbound>,
    mut observation_rx: watch::Receiver<Option<ClientOutbound>>,
    log_tx: Option<mpsc::Sender<WireRecord>>,
) where
    W: AsyncWrite + Unpin,
{
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut dirty = false;
    let mut flush_tick = tokio::time::interval(Duration::from_millis(16));
    flush_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        let msg = tokio::select! {
            biased;
            msg = reliable_rx.recv() => msg,
            changed = observation_rx.changed() => {
                if changed.is_err() {
                    None
                } else {
                    observation_rx.borrow_and_update().clone()
                }
            }
            _ = flush_tick.tick(), if dirty => {
                if writer.flush().await.is_err() {
                    break;
                }
                dirty = false;
                continue;
            }
        };

        let Some(msg) = msg else {
            break;
        };

        let flush_after = matches!(
            msg,
            ClientOutbound::Ack(_) | ClientOutbound::Error(_) | ClientOutbound::Welcome(_)
        );

        match msg {
            ClientOutbound::Ack(ack) => {
                if write_json_and_log(&mut writer, &mut buf, ack, log_tx.as_ref(), WireRecord::Ack)
                    .await
                    .is_err()
                {
                    break;
                }
            }
            ClientOutbound::Error(err) => {
                if write_json_and_log(
                    &mut writer,
                    &mut buf,
                    err,
                    log_tx.as_ref(),
                    WireRecord::Error,
                )
                .await
                .is_err()
                {
                    break;
                }
            }
            ClientOutbound::Welcome(welcome) => {
                if write_json_and_log(
                    &mut writer,
                    &mut buf,
                    welcome,
                    log_tx.as_ref(),
                    WireRecord::Welcome,
                )
                .await
                .is_err()
                {
                    break;
                }
            }
            ClientOutbound::ObservationArc(obs) => {
                if !encode_json_into_buf(&mut buf, obs.as_ref()) {
                    continue;
                }
                if writer.write_all(&buf).await.is_err() {
                    break;
                }
                log_wire_record(log_tx.as_ref(), WireRecord::ObservationArc(obs));
            }
        }

        if writer.write_all(b"\n").await.is_err() {
            break;
        }

        dirty = true;
        if flush_after {
            if writer.flush().await.is_err() {
                break;
            }
            dirty = false;
        }
    }
    let _ = writer.flush().await;
}
