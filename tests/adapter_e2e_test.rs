use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::protocol::{create_ack, create_hello};
use tui_tetris::adapter::runtime::InboundPayload;
use tui_tetris::adapter::server::{build_observation, run_server, ServerConfig};
use tui_tetris::adapter::{ClientCommand, InboundCommand, OutboundMessage};
use tui_tetris::core::GameState;
use tui_tetris::types::GameAction;

async fn recv_next_command(rx: &mut mpsc::Receiver<InboundCommand>) -> InboundCommand {
    loop {
        let inbound = rx.recv().await.expect("expected inbound message");
        if matches!(inbound.payload, InboundPayload::Command(_)) {
            return inbound;
        }
    }
}

#[tokio::test]
async fn adapter_wire_logging_writes_raw_frames() {
    let mut log_path = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    log_path.push(format!(
        "tui-tetris-wire-{}-{}.log",
        std::process::id(),
        stamp
    ));

    let _ = tokio::fs::remove_file(&log_path).await;

    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: Some(log_path.to_string_lossy().to_string()),
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
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
    let hello = create_hello(1, "wire-log-test", "2.0.0");
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

    drop(write_half);

    // Wait until the wire log contains both frames.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(contents) = tokio::fs::read_to_string(&log_path).await {
            if contents.contains("\"type\":\"hello\"")
                && contents.contains("\"type\":\"welcome\"")
            {
                // Ensure raw JSON lines only (no prefixes).
                for line in contents.lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    assert!(line.starts_with('{'));
                    assert!(line.ends_with('}'));
                }
                break;
            }
        }

        if tokio::time::Instant::now() >= deadline {
            let contents = tokio::fs::read_to_string(&log_path)
                .await
                .unwrap_or_default();
            panic!("wire log did not contain expected frames: {}", contents);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let _ = tokio::fs::remove_file(&log_path).await;
    server_handle.abort();
}

#[tokio::test]
async fn adapter_hello_command_ack_and_observation() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
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

    let inbound = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(inbound.seq, 2);
    match inbound.payload {
        InboundPayload::Command(ClientCommand::Actions(a)) => {
            assert_eq!(a, vec![GameAction::MoveLeft]);
        }
        _ => panic!("unexpected inbound payload"),
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
async fn adapter_hello_enqueues_snapshot_request() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .expect("server did not signal ready")
        .expect("ready channel dropped");

    let stream = TcpStream::connect(addr).await.expect("connect failed");
    let (_read_half, mut write_half) = stream.into_split();

    let hello = create_hello(1, "snapshot-test", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let inbound = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .expect("expected inbound message");
    assert_eq!(inbound.seq, 1);
    assert!(matches!(inbound.payload, InboundPayload::SnapshotRequest));

    server_handle.abort();
}

#[tokio::test]
async fn adapter_place_maps_to_place_command() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
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

    let hello = create_hello(1, "place-test", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap();

    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"place","place":{"x":3,"rotation":"east","useHold":false}}"#;
    write_half.write_all(cmd.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let inbound = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(inbound.seq, 2);
    match inbound.payload {
        InboundPayload::Command(ClientCommand::Place {
            x,
            rotation,
            use_hold,
        }) => {
            assert_eq!(x, 3);
            assert_eq!(rotation, tui_tetris::types::Rotation::East);
            assert!(!use_hold);
        }
        _ => panic!("expected place command"),
    }

    server_handle.abort();
}

#[tokio::test]
async fn adapter_place_invalid_rotation_returns_error() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
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

    let hello = create_hello(1, "place-test", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap();

    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"place","place":{"x":3,"rotation":"nope","useHold":false}}"#;
    write_half.write_all(cmd.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 2);
    assert_eq!(v["code"], "invalid_place");

    server_handle.abort();
}

#[tokio::test]
async fn adapter_backpressure_returns_error() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 1,
        log_path: None,
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
    let _first = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
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

#[tokio::test]
async fn adapter_requires_hello_before_command() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
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

    // command before hello
    let cmd = r#"{"type":"command","seq":1,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_half.write_all(cmd.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "handshake_required");

    server_handle.abort();
}

#[tokio::test]
async fn adapter_rejects_out_of_order_seq_after_hello() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
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

    // hello with seq=2
    let hello = create_hello(2, "seq-test", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let _welcome = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // Drain the hello-triggered snapshot request.
    let inbound = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .expect("expected inbound message");
    assert_eq!(inbound.seq, 2);
    assert!(matches!(inbound.payload, InboundPayload::SnapshotRequest));

    // command with seq=1 (out of order)
    let cmd = r#"{"type":"command","seq":1,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_half.write_all(cmd.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "invalid_command");

    // Ensure it was not enqueued.
    assert!(tokio::time::timeout(Duration::from_millis(100), recv_next_command(&mut cmd_rx))
        .await
        .is_err());

    server_handle.abort();
}

#[tokio::test]
async fn controller_disconnect_promotes_next_client() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Client 1 (becomes controller)
    let s1 = TcpStream::connect(addr).await.unwrap();
    let (r1, mut w1) = s1.into_split();
    let mut l1 = BufReader::new(r1).lines();
    let hello1 = create_hello(1, "c1", "2.0.0");
    w1.write_all(serde_json::to_string(&hello1).unwrap().as_bytes())
        .await
        .unwrap();
    w1.write_all(b"\n").await.unwrap();
    w1.flush().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), l1.next_line())
        .await
        .unwrap();

    // Client 2 (observer initially)
    let s2 = TcpStream::connect(addr).await.unwrap();
    let (r2, mut w2) = s2.into_split();
    let mut l2 = BufReader::new(r2).lines();
    let hello2 = create_hello(1, "c2", "2.0.0");
    w2.write_all(serde_json::to_string(&hello2).unwrap().as_bytes())
        .await
        .unwrap();
    w2.write_all(b"\n").await.unwrap();
    w2.flush().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), l2.next_line())
        .await
        .unwrap();

    // Drop client1 connection to trigger promotion.
    drop(w1);
    drop(l1);

    // Give server a moment to process disconnect.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Client2 sends a command; should now be accepted and queued.
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    w2.write_all(cmd.as_bytes()).await.unwrap();
    w2.write_all(b"\n").await.unwrap();
    w2.flush().await.unwrap();

    let inbound = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(inbound.seq, 2);

    server_handle.abort();
}

#[tokio::test]
async fn adapter_unknown_message_type_echoes_seq() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
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

    let hello = create_hello(1, "unknown-test", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap();

    // Unknown type with seq=9 should return invalid_command and echo seq.
    let msg = r#"{"type":"wat","seq":9,"ts":1}"#;
    write_half.write_all(msg.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 9);
    assert_eq!(v["code"], "invalid_command");

    server_handle.abort();
}
