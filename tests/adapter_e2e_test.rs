#![allow(clippy::field_reassign_with_default)] // Stepwise fixtures keep changed fields visible.

use std::io::{BufRead as _, Write as _};
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};

use tetris_adapter::adapter::game_loop::step_session;
use tetris_adapter::adapter::observation_schedule::ObservationSchedule;
use tetris_adapter::adapter::runtime::AdapterStatus;
use tetris_adapter::adapter::runtime::InboundPayload;
use tetris_adapter::adapter::server::{
    MAX_INBOUND_LINE_BYTES, ServerConfig, build_observation, run_server,
};
use tetris_adapter::adapter::{Adapter, ClientCommand, InboundCommand, OutboundMessage};
use tetris_adapter_protocol::protocol::TransitionEvent;
use tetris_adapter_protocol::protocol::create_hello;
use tetris_core::core::GameSnapshot;
use tetris_core::core::GameState;
use tetris_core::types::{CoreLastEvent, GameAction, TSpinKind};
use tetris_session::engine::session::SessionRuntime;

mod support;
use support::{read_json_line, spawn_server};

async fn recv_next_command(rx: &mut mpsc::Receiver<InboundCommand>) -> InboundCommand {
    loop {
        let inbound = rx.recv().await.expect("expected inbound message");
        if matches!(inbound.payload, InboundPayload::Command(_)) {
            return inbound;
        }
    }
}

fn read_std_json_line(reader: &mut std::io::BufReader<std::net::TcpStream>) -> serde_json::Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("adapter read failed");
    serde_json::from_str(&line).expect("adapter returned invalid JSON")
}

#[test]
fn adapter_start_reports_the_authoritative_bind_failure() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port,
        ..ServerConfig::default()
    };

    let error = match Adapter::start(config) {
        Ok(_) => panic!("adapter unexpectedly bound an occupied port"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("failed to bind AI adapter"));
}

#[test]
fn adapter_start_returns_the_actual_ephemeral_address() {
    let adapter = Adapter::start(ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        ..ServerConfig::default()
    })
    .unwrap();

    let address = adapter.listen_addr();
    assert_ne!(address.port(), 0);
}

#[test]
fn adapter_observation_event_scoring_fields_match_core_semantics() {
    // Case 1: combo=-1 must roundtrip ("no active combo chain").
    let mut snap = GameSnapshot::default();
    snap.score = 0;
    let event = CoreLastEvent {
        locked: true,
        lines_cleared: 0,
        line_clear_score: 0,
        tspin: None,
        combo: -1,
        back_to_back: false,
    };
    let obs = build_observation(1, 0, &snap, &[TransitionEvent::from(event)]);
    let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&obs).unwrap()).unwrap();
    assert_eq!(v["type"], "observation");
    assert_eq!(v["events"][0]["combo"], -1);
    assert_eq!(v["events"][0]["line_clear_score"], 0);
    assert_eq!(v["events"][0]["back_to_back"], false);

    // Case 2: line_clear_score is base-only (includes B2B; excludes combo + drop points).
    let mut snap = GameSnapshot::default();
    snap.score = 5450; // base (5400) + combo bonus (50)
    let event = CoreLastEvent {
        locked: true,
        lines_cleared: 4,
        line_clear_score: 5400,
        tspin: Some(TSpinKind::Full),
        combo: 1,
        back_to_back: true,
    };
    let obs = build_observation(2, 0, &snap, &[TransitionEvent::from(event)]);
    let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&obs).unwrap()).unwrap();
    assert_eq!(v["type"], "observation");
    assert_eq!(v["seq"], 2);
    assert_eq!(v["score"], 5450);
    assert_eq!(v["events"][0]["locked"], true);
    assert_eq!(v["events"][0]["lines_cleared"], 4);
    assert_eq!(v["events"][0]["line_clear_score"], 5400);
    assert_eq!(v["events"][0]["tspin"], "full");
    assert_eq!(v["events"][0]["combo"], 1);
    assert_eq!(v["events"][0]["back_to_back"], true);
}

#[test]
fn adapter_observation_tspin_no_line_clear_updates_score_without_event_tspin() {
    // T-Spin no-line points are awarded but not reported as a transition-event T-Spin.
    let mut snap = GameSnapshot::default();
    snap.score = 400 * (2 + 1);

    let event = CoreLastEvent {
        locked: true,
        lines_cleared: 0,
        line_clear_score: 0,
        tspin: None,
        combo: -1,
        back_to_back: false,
    };

    let obs = build_observation(3, 0, &snap, &[TransitionEvent::from(event)]);
    let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&obs).unwrap()).unwrap();
    assert_eq!(v["type"], "observation");
    assert_eq!(v["seq"], 3);
    assert_eq!(v["score"], 400 * 3);
    assert_eq!(v["events"][0]["lines_cleared"], 0);
    assert_eq!(v["events"][0]["line_clear_score"], 0);
    assert_eq!(v["events"][0]["combo"], -1);
    assert_eq!(v["events"][0]["back_to_back"], false);
    assert!(v["events"][0].get("tspin").is_none() || v["events"][0]["tspin"].is_null());
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
        protocol_version: "3.0.0".to_string(),
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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello
    let hello = create_hello(1, "wire-log-test", "3.0.0");
    let hello_line = serde_json::to_string(&hello).unwrap();
    support::write_raw_line(&mut write_half, hello_line).await;

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
        if let Ok(contents) = tokio::fs::read_to_string(&log_path).await
            && contents.contains("\"type\":\"hello\"")
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
    let config = support::server_config();

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello
    let hello = create_hello(1, "e2e-test", "3.0.0");
    let hello_line = serde_json::to_string(&hello).unwrap();
    support::write_raw_line(&mut write_half, hello_line).await;

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
    support::write_raw_line(&mut write_half, cmd_line).await;

    let inbound = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(inbound.seq, 2);
    match &inbound.payload {
        InboundPayload::Command(ClientCommand::Actions {
            actions,
            restart_seed,
        }) => {
            assert_eq!(actions.as_slice(), [GameAction::MoveLeft]);
            assert_eq!(*restart_seed, None);
        }
        _ => panic!("unexpected inbound payload"),
    }

    // Apply through the production protocol/session driver.
    let mut driver = tetris_adapter::adapter::game_loop::SessionProtocolDriver::new(1, 20);
    driver.handle(inbound);

    let ack_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected ack line");
    let ack_v: serde_json::Value = serde_json::from_str(&ack_line).unwrap();
    assert_eq!(ack_v["type"], "ack");
    assert_eq!(ack_v["seq"], 2);

    let obs_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("expected observation line");
    let obs_v: serde_json::Value = serde_json::from_str(&obs_line).unwrap();
    assert_eq!(obs_v["type"], "observation");
    assert_eq!(obs_v["type"], "observation");

    server_handle.abort();
}

#[tokio::test]
async fn adapter_broadcast_observation_arc_fanout() {
    let config = support::server_config();

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
    ) -> (
        tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        let (mut lines, mut write_half) = support::connect(addr).await;

        let hello = create_hello(1, name, "3.0.0");
        let hello_line = serde_json::to_string(&hello).unwrap();
        support::write_raw_line(&mut write_half, hello_line).await;

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
    let obs = build_observation(123, 0, &snap, &[]);

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

    server_handle.abort();
}

#[tokio::test]
async fn adapter_does_not_ack_until_game_loop_applies() {
    let config = support::server_config();

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello (disable snapshot request to keep cmd_rx predictable)
    let mut hello = create_hello(1, "ack-test", "3.0.0");
    hello.requested.stream_observations = false;
    support::write_json_line(&mut write_half, &hello).await;

    let _welcome = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // command
    let cmd_line = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_half, cmd_line).await;

    let inbound = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(inbound.seq, 2);

    // No ack should be emitted until the game loop applies and sends it.
    assert!(
        tokio::time::timeout(Duration::from_millis(150), lines.next_line())
            .await
            .is_err()
    );

    let mut driver = tetris_adapter::adapter::game_loop::SessionProtocolDriver::new(1, 20);
    driver.handle(inbound);

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
    let config = support::server_config();

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();
    let (status_tx, mut status_rx) = watch::channel(AdapterStatus {
        client_count: 0,
        controller_id: None,
        streaming_count: 0,
    });

    let server_handle = tokio::spawn(async move {
        run_server(config, cmd_tx, out_rx, Some(ready_tx), Some(status_tx))
            .await
            .expect("adapter status test server failed");
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Initial status (0 clients).
    tokio::time::timeout(Duration::from_secs(2), status_rx.changed())
        .await
        .unwrap()
        .expect("status channel closed");
    let st0 = *status_rx.borrow_and_update();
    assert_eq!(st0.client_count, 0);
    assert_eq!(st0.streaming_count, 0);

    let (mut lines, mut write_half) = support::connect(addr).await;

    // Wait until we see client_count >= 1.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(Ok(())) =
            tokio::time::timeout(Duration::from_millis(50), status_rx.changed()).await
        {
            let st = *status_rx.borrow_and_update();
            if st.client_count >= 1 {
                break;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("did not observe client_count >= 1");
        }
    }

    // hello makes this client controller.
    let hello = create_hello(1, "status-test", "3.0.0");
    support::write_json_line(&mut write_half, &hello).await;

    let _welcome_line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // Status should eventually show controller_id set and stream_observations enabled.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(Ok(())) =
            tokio::time::timeout(Duration::from_millis(50), status_rx.changed()).await
        {
            let st = *status_rx.borrow_and_update();
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
    let config = support::server_config();

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

    let hello = create_hello(1, "snapshot-test", "3.0.0");
    support::write_json_line(&mut write_half, &hello).await;

    let inbound = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .expect("expected inbound message");
    assert_eq!(inbound.seq, 1);
    assert!(matches!(inbound.payload, InboundPayload::SnapshotRequest));

    server_handle.abort();
}

#[tokio::test]
async fn adapter_hello_snapshot_request_waits_for_bounded_queue_capacity() {
    let config = support::server_config_with_capacity(1);
    let (server, addr, mut cmd_rx, _out_tx) = spawn_server(config, 1).await;

    let mut clients = Vec::new();
    for name in ["snapshot-a", "snapshot-b"] {
        let (mut lines, mut write_half) = support::connect(addr).await;
        let hello = create_hello(1, name, "3.0.0");
        support::write_json_line(&mut write_half, &hello).await;
        let welcome = read_json_line(&mut lines).await;
        assert_eq!(welcome["type"], "welcome");
        clients.push((write_half, lines));
    }

    let first = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .expect("first snapshot request");
    assert!(matches!(first.payload, InboundPayload::SnapshotRequest));

    let second = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .expect("second snapshot request");
    assert!(matches!(second.payload, InboundPayload::SnapshotRequest));

    drop(clients);
    server.abort();
}

#[test]
fn production_session_replies_through_the_originating_client_mailbox() {
    let config = support::server_config_with_capacity(8);
    let mut adapter = Some(Adapter::start(config).unwrap());
    let addr = adapter.as_ref().unwrap().listen_addr();
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());

    let hello = create_hello(1, "production-session", "3.0.0");
    stream
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();
    assert_eq!(read_std_json_line(&mut reader)["type"], "welcome");

    std::thread::sleep(Duration::from_millis(10));
    let mut session = SessionRuntime::new(1);
    let mut observations = ObservationSchedule::new(session.game(), 20);
    step_session(&mut adapter, &mut session, &mut observations, &[], true);
    assert_eq!(read_std_json_line(&mut reader)["type"], "observation");

    stream
        .write_all(br#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#)
        .unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();
    std::thread::sleep(Duration::from_millis(10));
    step_session(&mut adapter, &mut session, &mut observations, &[], true);

    let ack = read_std_json_line(&mut reader);
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["seq"], 2);
}

#[tokio::test]
async fn adapter_place_maps_to_place_command() {
    let config = support::server_config_with_capacity(8);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    let hello = create_hello(1, "place-test", "3.0.0");
    support::write_json_line(&mut write_half, &hello).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap();

    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"place","place":{"x":3,"rotation":"east","useHold":false}}"#;
    support::write_raw_line(&mut write_half, cmd).await;

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
            assert_eq!(rotation, tetris_core::types::Rotation::East);
            assert!(!use_hold);
        }
        _ => panic!("expected place command"),
    }

    server_handle.abort();
}

#[tokio::test]
async fn adapter_backpressure_returns_error() {
    let config = support::server_config_with_capacity(1);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    let mut hello = create_hello(1, "e2e-test", "3.0.0");
    // Keep the inbound command queue empty (hello snapshot would fill it).
    hello.requested.stream_observations = false;
    support::write_json_line(&mut write_half, &hello).await;
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
    support::write_raw_line(&mut write_half, cmd2).await;

    // Read the error before draining the queue, keeping the backpressure state deterministic.
    let mut got_backpressure = false;
    let mut got_retry_after = false;
    for _ in 0..10 {
        let next = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap();
        let line = next.unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["type"] == "error" && v["seq"] == 3 && v["code"] == "backpressure" {
            got_backpressure = true;
            got_retry_after = v
                .get("retry_after_ms")
                .and_then(|x| x.as_u64())
                .is_some_and(|n| n >= 1);
            break;
        }
    }
    assert!(got_backpressure);
    assert!(got_retry_after);

    // First should be queued.
    let first = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(first.seq, 2);

    // Ensure the backpressured command was NOT enqueued.
    assert!(
        tokio::time::timeout(Duration::from_millis(100), recv_next_command(&mut cmd_rx))
            .await
            .is_err()
    );

    // Retry with a new, larger seq after draining the queue should succeed.
    let cmd3 = r#"{"type":"command","seq":4,"ts":1,"mode":"action","actions":["moveRight"]}"#;
    support::write_raw_line(&mut write_half, cmd3).await;

    let retried = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(retried.seq, 4);

    server_handle.abort();
}

#[tokio::test]
async fn adapter_observation_timers_roundtrip_over_tcp() {
    let config = support::server_config();

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    let hello = create_hello(1, "timers-roundtrip", "3.0.0");
    support::write_json_line(&mut write_half, &hello).await;

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
    let obs = build_observation(200, 0, &snap, &[]);

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
async fn adapter_parse_error_echoes_seq_best_effort() {
    let config = support::server_config_with_capacity(8);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // Invalid JSON but includes seq=9.
    let bad = r#"{"type":"command","seq":9,"ts":1,"mode":"action","actions":["moveLeft"]"#;
    support::write_raw_line(&mut write_half, bad).await;

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
async fn controller_disconnect_promotes_next_client() {
    let config = support::server_config();

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();
    let (status_tx, mut status_rx) = watch::channel(AdapterStatus {
        client_count: 0,
        controller_id: None,
        streaming_count: 0,
    });

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), Some(status_tx)).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Client 1 (becomes controller)
    let (mut l1, mut w1) = support::connect(addr).await;
    let hello1 = create_hello(1, "c1", "3.0.0");
    support::write_json_line(&mut w1, &hello1).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), l1.next_line())
        .await
        .unwrap();

    // Client 2 (observer initially)
    let (mut l2, mut w2) = support::connect(addr).await;
    let hello2 = create_hello(1, "c2", "3.0.0");
    support::write_json_line(&mut w2, &hello2).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), l2.next_line())
        .await
        .unwrap();

    // Invalid UTF-8 exercises the error cleanup path, not only clean EOF.
    w1.write_all(&[0xff, b'\n']).await.unwrap();
    w1.flush().await.unwrap();
    drop(w1);
    drop(l1);

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if status_rx.borrow().controller_id == Some(2) {
                break;
            }
            status_rx.changed().await.unwrap();
        }
    })
    .await
    .expect("controller promotion timed out");

    // Client2 sends a command; should now be accepted and queued.
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut w2, cmd).await;

    let inbound = tokio::time::timeout(Duration::from_secs(2), recv_next_command(&mut cmd_rx))
        .await
        .unwrap();
    assert_eq!(inbound.seq, 2);

    server_handle.abort();
}

#[tokio::test]
async fn adapter_unknown_message_type_echoes_seq() {
    let config = support::server_config();

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    let hello = create_hello(1, "unknown-test", "3.0.0");
    support::write_json_line(&mut write_half, &hello).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap();

    // Unknown type with seq=9 should return invalid_command and echo seq.
    let msg = r#"{"type":"wat","seq":9,"ts":1}"#;
    support::write_raw_line(&mut write_half, msg).await;

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

#[tokio::test]
async fn adapter_disconnects_frames_that_exceed_the_inbound_limit() {
    let config = support::server_config();
    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (_out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();
    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let addr = ready_rx.await.expect("server ready");

    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let oversized = vec![b' '; MAX_INBOUND_LINE_BYTES + 1];
    let _ = stream.write_all(&oversized).await;
    let _ = stream.shutdown().await;

    let mut byte = [0u8; 1];
    match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte)).await {
        Ok(Ok(0)) | Ok(Err(_)) => {}
        Ok(Ok(count)) => panic!("server wrote {count} bytes instead of closing oversized frame"),
        Err(_) => panic!("server did not close oversized frame"),
    }

    server_handle.abort();
}
