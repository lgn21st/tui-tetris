use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::protocol::{create_ack, create_hello};
use tui_tetris::adapter::protocol::LastEvent;
use tui_tetris::adapter::runtime::InboundPayload;
use tui_tetris::adapter::runtime::AdapterStatus;
use tui_tetris::adapter::server::{build_observation, run_server, ServerConfig};
use tui_tetris::adapter::{ClientCommand, InboundCommand, OutboundMessage};
use tui_tetris::core::GameSnapshot;
use tui_tetris::core::GameState;
use tui_tetris::types::{CoreLastEvent, GameAction, TSpinKind};

async fn recv_next_command(rx: &mut mpsc::Receiver<InboundCommand>) -> InboundCommand {
    loop {
        let inbound = rx.recv().await.expect("expected inbound message");
        if matches!(inbound.payload, InboundPayload::Command(_)) {
            return inbound;
        }
    }
}

#[test]
fn adapter_observation_last_event_scoring_fields_match_core_semantics() {
    // Case 1: combo=-1 must roundtrip (swiftui-tetris uses -1 as "no active combo chain").
    let mut snap = GameSnapshot::default();
    snap.score = 0;
    let last_event = CoreLastEvent {
        locked: true,
        lines_cleared: 0,
        line_clear_score: 0,
        tspin: None,
        combo: -1,
        back_to_back: false,
    };
    let obs = build_observation(1, &snap, Some(LastEvent::from(last_event)));
    let v: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&obs).unwrap()).unwrap();
    assert_eq!(v["type"], "observation");
    assert_eq!(v["last_event"]["combo"], -1);
    assert_eq!(v["last_event"]["line_clear_score"], 0);
    assert_eq!(v["last_event"]["back_to_back"], false);

    // Case 2: line_clear_score is base-only (includes B2B; excludes combo + drop points).
    let mut snap = GameSnapshot::default();
    snap.score = 5450; // base (5400) + combo bonus (50)
    let last_event = CoreLastEvent {
        locked: true,
        lines_cleared: 4,
        line_clear_score: 5400,
        tspin: Some(TSpinKind::Full),
        combo: 1,
        back_to_back: true,
    };
    let obs = build_observation(2, &snap, Some(LastEvent::from(last_event)));
    let v: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&obs).unwrap()).unwrap();
    assert_eq!(v["type"], "observation");
    assert_eq!(v["seq"], 2);
    assert_eq!(v["score"], 5450);
    assert_eq!(v["last_event"]["locked"], true);
    assert_eq!(v["last_event"]["lines_cleared"], 4);
    assert_eq!(v["last_event"]["line_clear_score"], 5400);
    assert_eq!(v["last_event"]["tspin"], "full");
    assert_eq!(v["last_event"]["combo"], 1);
    assert_eq!(v["last_event"]["back_to_back"], true);
}

#[test]
fn adapter_observation_tspin_no_line_clear_updates_score_without_last_event_tspin() {
    // swiftui-tetris awards T-Spin no-line points but does not report it as a `last_event` T-Spin.
    let mut snap = GameSnapshot::default();
    snap.score = 400 * (2 + 1);

    let last_event = CoreLastEvent {
        locked: true,
        lines_cleared: 0,
        line_clear_score: 0,
        tspin: None,
        combo: -1,
        back_to_back: false,
    };

    let obs = build_observation(3, &snap, Some(LastEvent::from(last_event)));
    let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&obs).unwrap()).unwrap();
    assert_eq!(v["type"], "observation");
    assert_eq!(v["seq"], 3);
    assert_eq!(v["score"], 400 * 3);
    assert_eq!(v["last_event"]["lines_cleared"], 0);
    assert_eq!(v["last_event"]["line_clear_score"], 0);
    assert_eq!(v["last_event"]["combo"], -1);
    assert_eq!(v["last_event"]["back_to_back"], false);
    assert!(v["last_event"].get("tspin").is_none() || v["last_event"]["tspin"].is_null());
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
        ..ServerConfig::default()
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
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
        ..ServerConfig::default()
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
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
            assert_eq!(a.as_slice(), [GameAction::MoveLeft]);
        }
        _ => panic!("unexpected inbound payload"),
    }

    // ack after apply
    let ack = create_ack(2, 2);
    out_tx
        .send(OutboundMessage::ToClientAck {
            client_id: inbound.client_id,
            ack,
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
    let snap = gs.snapshot();
    let obs = build_observation(10, &snap, None);
    out_tx
        .send(OutboundMessage::BroadcastArc {
            line: std::sync::Arc::from(serde_json::to_string(&obs).unwrap()),
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
async fn adapter_broadcast_observation_arc_fanout() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        ..ServerConfig::default()
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .expect("server did not signal ready")
        .expect("ready channel dropped");

    async fn connect_and_handshake(
        addr: std::net::SocketAddr,
        name: &str,
    ) -> (tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>, tokio::net::tcp::OwnedWriteHalf) {
        let stream = TcpStream::connect(addr).await.expect("connect failed");
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let hello = create_hello(1, name, "2.0.0");
        let hello_line = serde_json::to_string(&hello).unwrap();
        write_half.write_all(hello_line.as_bytes()).await.unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        // welcome
        let welcome_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .expect("expected welcome line");
        let welcome_v: serde_json::Value = serde_json::from_str(&welcome_line).unwrap();
        assert_eq!(welcome_v["type"], "welcome");

        (lines, write_half)
    }

    let (mut lines_a, _write_a) = connect_and_handshake(addr, "fanout-a").await;
    let (mut lines_b, _write_b) = connect_and_handshake(addr, "fanout-b").await;

    let mut gs = GameState::new(1);
    gs.start();
    let snap = gs.snapshot();
    let obs = build_observation(123, &snap, None);

    out_tx
        .send(OutboundMessage::BroadcastObservationArc { obs: Arc::new(obs) })
        .unwrap();

    let line_a = tokio::time::timeout(Duration::from_secs(2), lines_a.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected broadcast observation for a");
    let v_a: serde_json::Value = serde_json::from_str(&line_a).unwrap();
    assert_eq!(v_a["type"], "observation");

    let line_b = tokio::time::timeout(Duration::from_secs(2), lines_b.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected broadcast observation for b");
    let v_b: serde_json::Value = serde_json::from_str(&line_b).unwrap();
    assert_eq!(v_b["type"], "observation");

    // Also ensure the non-Arc broadcast variant fans out correctly (backward compatible).
    let obs2 = build_observation(124, &snap, None);
    out_tx
        .send(OutboundMessage::BroadcastObservation { obs: obs2 })
        .unwrap();

    let line_a2 = tokio::time::timeout(Duration::from_secs(2), lines_a.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected broadcast observation for a (non-Arc)");
    let v_a2: serde_json::Value = serde_json::from_str(&line_a2).unwrap();
    assert_eq!(v_a2["type"], "observation");
    assert_eq!(v_a2["seq"], 124);

    let line_b2 = tokio::time::timeout(Duration::from_secs(2), lines_b.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected broadcast observation for b (non-Arc)");
    let v_b2: serde_json::Value = serde_json::from_str(&line_b2).unwrap();
    assert_eq!(v_b2["type"], "observation");
    assert_eq!(v_b2["seq"], 124);

    // Also ensure string line Arc variants work (they are used by some harnesses/tools).
    let ping: Arc<str> = Arc::from(r#"{"type":"ack","seq":999,"ts":1,"status":"ok"}"#);
    out_tx
        .send(OutboundMessage::BroadcastArc {
            line: Arc::clone(&ping),
        })
        .unwrap();

    let line_a3 = tokio::time::timeout(Duration::from_secs(2), lines_a.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected broadcast line for a (Arc)");
    let v_a3: serde_json::Value = serde_json::from_str(&line_a3).unwrap();
    assert_eq!(v_a3["type"], "ack");
    assert_eq!(v_a3["seq"], 999);

    let line_b3 = tokio::time::timeout(Duration::from_secs(2), lines_b.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected broadcast line for b (Arc)");
    let v_b3: serde_json::Value = serde_json::from_str(&line_b3).unwrap();
    assert_eq!(v_b3["type"], "ack");
    assert_eq!(v_b3["seq"], 999);

    out_tx
        .send(OutboundMessage::ToClientArc {
            client_id: 0,
            line: Arc::from(r#"{"type":"ack","seq":1000,"ts":1,"status":"ok"}"#),
        })
        .unwrap();
    out_tx
        .send(OutboundMessage::ToClientArc {
            client_id: 1,
            line: Arc::from(r#"{"type":"ack","seq":1001,"ts":1,"status":"ok"}"#),
        })
        .unwrap();

    let mut got = false;
    if let Ok(Ok(Some(line))) =
        tokio::time::timeout(Duration::from_millis(200), lines_a.next_line()).await
    {
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["type"] == "ack" && (v["seq"] == 1000 || v["seq"] == 1001) {
            got = true;
        }
    }
    if let Ok(Ok(Some(line))) =
        tokio::time::timeout(Duration::from_millis(200), lines_b.next_line()).await
    {
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["type"] == "ack" && (v["seq"] == 1000 || v["seq"] == 1001) {
            got = true;
        }
    }
    assert!(got);

    server_handle.abort();
}

#[tokio::test]
async fn adapter_does_not_ack_until_game_loop_applies() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.expect("connect failed");
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello (disable snapshot request to keep cmd_rx predictable)
    let mut hello = create_hello(1, "ack-test", "2.0.0");
    hello.requested.stream_observations = false;
    write_half.write_all(serde_json::to_string(&hello).unwrap().as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let _welcome = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // command
    let cmd_line = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_half.write_all(cmd_line.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let inbound = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(inbound.seq, 2);

    // No ack should be emitted until the game loop applies and sends it.
    assert!(tokio::time::timeout(Duration::from_millis(150), lines.next_line())
        .await
        .is_err());

    // Simulate game loop apply -> send ack.
    let ack = create_ack(2, 2);
    out_tx
        .send(OutboundMessage::ToClientAck {
            client_id: inbound.client_id,
            ack,
        })
        .unwrap();

    let ack_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&ack_line).unwrap();
    assert_eq!(v["type"], "ack");
    assert_eq!(v["seq"], 2);

    server_handle.abort();
}


#[tokio::test]
async fn adapter_emits_status_updates_on_connect_and_controller() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<AdapterStatus>();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), Some(status_tx)).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Initial status (0 clients).
    let st0 = tokio::time::timeout(Duration::from_secs(2), status_rx.recv())
        .await
        .unwrap()
        .expect("status channel closed");
    assert_eq!(st0.client_count, 0);
    assert_eq!(st0.streaming_count, 0);

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // Wait until we see client_count >= 1.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(Some(st)) = tokio::time::timeout(Duration::from_millis(50), status_rx.recv()).await {
            if st.client_count >= 1 {
                break;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("did not observe client_count >= 1");
        }
    }

    // hello makes this client controller.
    let hello = create_hello(1, "status-test", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let _welcome_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // Status should eventually show controller_id set and stream_observations enabled.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(Some(st)) = tokio::time::timeout(Duration::from_millis(50), status_rx.recv()).await {
            if st.controller_id.is_some() && st.streaming_count >= 1 {
                break;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("did not observe controller_id set and streaming_count >= 1");
        }
    }

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
        ..ServerConfig::default()
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
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
        log_every_n: 1,
        log_max_lines: None,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
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
        log_every_n: 1,
        log_max_lines: None,
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
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
        ..ServerConfig::default()
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(1);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let mut hello = create_hello(1, "e2e-test", "2.0.0");
    // Keep the inbound command queue empty (hello snapshot would fill it).
    hello.requested.stream_observations = false;
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

    // Allow the server to process both commands while the bounded queue is still full.
    // If we drain `cmd_rx` too early, `cmd2` may be enqueued instead of backpressured.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Expect an error for seq=3.
    let mut got_backpressure = false;
    for _ in 0..10 {
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

    // First should be queued.
    let first = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(first.seq, 2);

    // Ensure the backpressured command was NOT enqueued.
    assert!(tokio::time::timeout(Duration::from_millis(100), recv_next_command(&mut cmd_rx))
        .await
        .is_err());

    // Retry with a new, larger seq after draining the queue should succeed.
    let cmd3 = r#"{"type":"command","seq":4,"ts":1,"mode":"action","actions":["moveRight"]}"#;
    write_half.write_all(cmd3.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let retried = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(retried.seq, 4);


    server_handle.abort();
}

#[tokio::test]
async fn adapter_observation_timers_roundtrip_over_tcp() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        ..ServerConfig::default()
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .expect("server did not signal ready")
        .expect("ready channel dropped");

    let stream = TcpStream::connect(addr).await.expect("connect failed");
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let hello = create_hello(1, "timers-roundtrip", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    // welcome
    let welcome_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected welcome line");
    let welcome_v: serde_json::Value = serde_json::from_str(&welcome_line).unwrap();
    assert_eq!(welcome_v["type"], "welcome");

    let mut snap = GameSnapshot::default();
    snap.timers.drop_ms = 12;
    snap.timers.lock_ms = 34;
    snap.timers.line_clear_ms = 56;
    let obs = build_observation(200, &snap, None);

    out_tx
        .send(OutboundMessage::BroadcastObservationArc { obs: Arc::new(obs) })
        .unwrap();

    let obs_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected observation line");
    let obs_v: serde_json::Value = serde_json::from_str(&obs_line).unwrap();
    assert_eq!(obs_v["type"], "observation");
    assert_eq!(obs_v["seq"], 200);
    assert_eq!(obs_v["timers"]["drop_ms"], 12);
    assert_eq!(obs_v["timers"]["lock_ms"], 34);
    assert_eq!(obs_v["timers"]["line_clear_ms"], 56);

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
        log_every_n: 1,
        log_max_lines: None,
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
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
async fn adapter_parse_error_echoes_seq_best_effort() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        log_every_n: 1,
        log_max_lines: None,
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // Invalid JSON but includes seq=9.
    let bad = r#"{"type":"command","seq":9,"ts":1,"mode":"action","actions":["moveLeft"]"#;
    write_half.write_all(bad.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["code"], "invalid_command");
    assert_eq!(v["seq"], 9);

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
        log_every_n: 1,
        log_max_lines: None,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello with seq=1
    let hello = create_hello(1, "seq-test", "2.0.0");
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
    assert_eq!(inbound.seq, 1);
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
async fn adapter_rejects_hello_seq_not_one() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        log_every_n: 1,
        log_max_lines: None,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello with seq!=1 must be rejected.
    let hello = create_hello(2, "seq-test", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
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
    assert_eq!(v["code"], "invalid_command");

    // Ensure it did not trigger snapshot request.
    assert!(tokio::time::timeout(Duration::from_millis(100), cmd_rx.recv())
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
        log_every_n: 1,
        log_max_lines: None,
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
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
        ..ServerConfig::default()
    };

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
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
