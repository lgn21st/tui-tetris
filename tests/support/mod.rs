#![allow(dead_code)] // Each integration-test crate uses a different subset of shared helpers.

use std::net::SocketAddr;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot};

use tetris_adapter::adapter::server::{ServerConfig, run_server};
use tetris_adapter::adapter::{InboundCommand, OutboundMessage};

pub type ClientLines = tokio::io::Lines<BufReader<OwnedReadHalf>>;

pub fn server_config() -> ServerConfig {
    ServerConfig {
        port: 0,
        max_pending_commands: 8,
        ..ServerConfig::default()
    }
}

pub fn server_config_with_capacity(max_pending_commands: usize) -> ServerConfig {
    ServerConfig {
        max_pending_commands,
        ..server_config()
    }
}

pub async fn connect(address: SocketAddr) -> (ClientLines, OwnedWriteHalf) {
    let (reader, writer) = TcpStream::connect(address)
        .await
        .expect("adapter test client failed to connect")
        .into_split();
    (BufReader::new(reader).lines(), writer)
}

pub async fn write_json_line<T: Serialize + ?Sized>(writer: &mut OwnedWriteHalf, value: &T) {
    let bytes = serde_json::to_vec(value).expect("test message must serialize");
    writer
        .write_all(&bytes)
        .await
        .expect("adapter test write failed");
    writer
        .write_all(b"\n")
        .await
        .expect("adapter test newline write failed");
    writer.flush().await.expect("adapter test flush failed");
}

pub async fn write_raw_line(writer: &mut OwnedWriteHalf, value: impl AsRef<[u8]>) {
    writer
        .write_all(value.as_ref())
        .await
        .expect("adapter test write failed");
    writer
        .write_all(b"\n")
        .await
        .expect("adapter test newline write failed");
    writer.flush().await.expect("adapter test flush failed");
}

pub async fn read_line(lines: &mut ClientLines) -> String {
    tokio::time::timeout(Duration::from_secs(3), lines.next_line())
        .await
        .expect("timeout waiting for adapter line")
        .expect("adapter read failed")
        .expect("adapter closed before expected line")
}

pub async fn read_json_line(lines: &mut ClientLines) -> serde_json::Value {
    serde_json::from_str(&read_line(lines).await).expect("adapter returned invalid JSON")
}

pub async fn spawn_server(
    config: ServerConfig,
    command_capacity: usize,
) -> (
    tokio::task::JoinHandle<()>,
    SocketAddr,
    mpsc::Receiver<InboundCommand>,
    mpsc::UnboundedSender<OutboundMessage>,
) {
    let (command_tx, command_rx) = mpsc::channel(command_capacity);
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server = tokio::spawn(async move {
        run_server(config, command_tx, outbound_rx, Some(ready_tx), None)
            .await
            .expect("adapter test server failed");
    });
    let address = tokio::time::timeout(Duration::from_secs(3), ready_rx)
        .await
        .expect("adapter server start timed out")
        .expect("adapter server did not report its address");

    (server, address, command_rx, outbound_tx)
}
