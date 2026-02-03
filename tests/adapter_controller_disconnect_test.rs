use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::protocol::{create_hello, CommandMode};
use tui_tetris::adapter::runtime::InboundPayload;
use tui_tetris::adapter::server::{run_server, ServerConfig};
use tui_tetris::adapter::{InboundCommand, OutboundMessage};

async fn read_line(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
) -> String {
    tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .expect("timeout waiting for line")
        .expect("io error")
        .expect("expected line")
}

#[tokio::test]
async fn controller_disconnect_does_not_leave_stale_controller() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 64,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(128);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    // Minimal engine loop: ack every command so the client can observe controller gating.
    let engine_handle = tokio::spawn(async move {
        while let Some(inbound) = cmd_rx.recv().await {
            if matches!(inbound.payload, InboundPayload::Command(_)) {
                let ack = tui_tetris::adapter::protocol::create_ack(inbound.seq, inbound.seq);
                let _ = out_tx.send(OutboundMessage::ToClientAck {
                    client_id: inbound.client_id,
                    ack,
                });
            }
        }
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Client 1 becomes controller on hello and then disconnects via RST.
    {
        let stream = TcpStream::connect(addr).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let mut hello = create_hello(1, "ctrl1", "2.0.0");
        hello.requested.stream_observations = false;
        hello.requested.command_mode = CommandMode::Action;
        write_half
            .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
            .await
            .unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let welcome: serde_json::Value = serde_json::from_str(&read_line(&mut lines).await).unwrap();
        assert_eq!(welcome["type"], "welcome");

        // Send an invalid UTF-8 line to force a server-side read error in the line reader.
        // This exercises the disconnect/cleanup path even when the socket ends with an I/O error
        // (not just a clean EOF), which previously could leave a stale controller.
        write_half.write_all(&[0xFF, b'\n']).await.unwrap();
        let _ = write_half.flush().await;
    }

    // Give the server a moment to observe the disconnect and run cleanup.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Client 2 should be able to control after client 1 disconnect.
    {
        let stream = TcpStream::connect(addr).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let mut hello = create_hello(1, "ctrl2", "2.0.0");
        hello.requested.stream_observations = false;
        hello.requested.command_mode = CommandMode::Action;
        write_half
            .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
            .await
            .unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let welcome: serde_json::Value = serde_json::from_str(&read_line(&mut lines).await).unwrap();
        assert_eq!(welcome["type"], "welcome");

        let cmd = serde_json::json!({
            "type": "command",
            "seq": 2,
            "ts": 1,
            "mode": "action",
            "actions": ["hardDrop"]
        });
        write_half
            .write_all(serde_json::to_string(&cmd).unwrap().as_bytes())
            .await
            .unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let resp: serde_json::Value = serde_json::from_str(&read_line(&mut lines).await).unwrap();
        assert_eq!(resp["type"], "ack", "expected ack, got {resp}");
        assert_eq!(resp["seq"], 2);
    }

    server_handle.abort();
    engine_handle.abort();
}
