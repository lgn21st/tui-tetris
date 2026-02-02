use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::protocol::{create_ack, create_hello};
use tui_tetris::adapter::server::{build_observation, run_server, ServerConfig};
use tui_tetris::adapter::{ClientCommand, InboundCommand, OutboundMessage};
use tui_tetris::core::GameState;
use tui_tetris::types::GameAction;

#[tokio::test]
async fn adapter_hello_command_ack_and_observation() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .expect("server did not signal ready")
        .expect("ready channel dropped");

    let stream = TcpStream::connect(addr).await.expect("connect failed");
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello
    let hello = create_hello(1, "e2e-test", "2.0.0");
    let hello_line = serde_json::to_string(&hello).unwrap();
    write_half.write_all(hello_line.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let welcome_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected welcome line");
    let welcome_v: serde_json::Value = serde_json::from_str(&welcome_line).unwrap();
    assert_eq!(welcome_v["type"], "welcome");
    assert_eq!(welcome_v["seq"], 1);

    // command
    let cmd_line = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_half.write_all(cmd_line.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let inbound = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .expect("expected inbound command");
    assert_eq!(inbound.seq, 2);
    match inbound.command {
        ClientCommand::Actions(a) => {
            assert_eq!(a, vec![GameAction::MoveLeft]);
        }
        _ => panic!("unexpected command type"),
    }

    // ack after apply
    let ack = create_ack(2, 2);
    out_tx
        .send(OutboundMessage::ToClient {
            client_id: inbound.client_id,
            line: serde_json::to_string(&ack).unwrap(),
        })
        .unwrap();

    let ack_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected ack line");
    let ack_v: serde_json::Value = serde_json::from_str(&ack_line).unwrap();
    assert_eq!(ack_v["type"], "ack");
    assert_eq!(ack_v["seq"], 2);

    // broadcast observation
    let mut gs = GameState::new(1);
    gs.start();
    let obs = build_observation(&gs, 10, 0, 1, 0, None);
    out_tx
        .send(OutboundMessage::Broadcast {
            line: serde_json::to_string(&obs).unwrap(),
        })
        .unwrap();

    let obs_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected observation line");
    let obs_v: serde_json::Value = serde_json::from_str(&obs_line).unwrap();
    assert_eq!(obs_v["type"], "observation");
    assert_eq!(obs_v["seq"], 10);

    server_handle.abort();
}

#[tokio::test]
async fn adapter_backpressure_returns_error() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 1,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(1);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let hello = create_hello(1, "e2e-test", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    // welcome
    let _ = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap();

    // Send two commands without draining cmd_rx; second should backpressure.
    let cmd1 = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    let cmd2 = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["moveRight"]}"#;
    write_half.write_all(cmd1.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.write_all(cmd2.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    // First should be queued.
    let _first = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .unwrap();

    // Expect an error for seq=3.
    let mut got_backpressure = false;
    for _ in 0..5 {
        let next = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap();
        let line = next.unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["type"] == "error" && v["seq"] == 3 && v["code"] == "backpressure" {
            got_backpressure = true;
            break;
        }
    }
    assert!(got_backpressure);

    server_handle.abort();
}
