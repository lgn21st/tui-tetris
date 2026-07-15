#![allow(dead_code)] // Each integration-test crate uses a different subset of shared helpers.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::BufReader;
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::server::{run_server, ServerConfig};
use tui_tetris::adapter::{InboundCommand, OutboundMessage};

pub type ClientLines = tokio::io::Lines<BufReader<OwnedReadHalf>>;

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
