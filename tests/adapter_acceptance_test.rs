use std::time::Duration;
use std::net::SocketAddr;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::protocol::{create_ack, create_error, create_hello, ErrorCode};
use tui_tetris::adapter::runtime::InboundPayload;
use tui_tetris::adapter::server::{build_observation, run_server, ServerConfig};
use tui_tetris::adapter::{ClientCommand, InboundCommand, OutboundMessage};
use tui_tetris::core::GameState;
use tui_tetris::types::GameAction;

async fn read_json_line(lines: &mut tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>) -> serde_json::Value {
    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .expect("timeout waiting for line")
        .expect("io error")
        .expect("expected line");
    serde_json::from_str(&line).expect("invalid json")
}


async fn spawn_server(
    config: ServerConfig,
    cmd_capacity: usize,
) -> (
    tokio::task::JoinHandle<()>,
    SocketAddr,
    mpsc::Receiver<InboundCommand>,
    mpsc::UnboundedSender<OutboundMessage>,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(cmd_capacity);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    (server_handle, addr, cmd_rx, out_tx)
}

async fn engine_task(mut cmd_rx: mpsc::Receiver<InboundCommand>, out_tx: mpsc::UnboundedSender<OutboundMessage>) {
    let mut gs = GameState::new(1);
    gs.start();
    let mut obs_seq: u64 = 100;

    while let Some(inbound) = cmd_rx.recv().await {
        match inbound.payload {
            InboundPayload::SnapshotRequest => {
                let last_event = gs
                    .take_last_event()
                    .map(tui_tetris::adapter::protocol::LastEvent::from);
                let snap = gs.snapshot();
                let obs = build_observation(obs_seq, &snap, last_event);
                obs_seq += 1;
                let line: std::sync::Arc<str> =
                    std::sync::Arc::from(serde_json::to_string(&obs).unwrap());
                let _ = out_tx.send(OutboundMessage::ToClientArc {
                    client_id: inbound.client_id,
                    line,
                });
            }
            InboundPayload::Command(cmd) => {
                match cmd {
                    ClientCommand::Actions(actions) => {
                        for a in actions {
                            let _ = gs.apply_action(a);
                        }
                        let ack = create_ack(inbound.seq, inbound.seq);
                        let line: std::sync::Arc<str> =
                            std::sync::Arc::from(serde_json::to_string(&ack).unwrap());
                        let _ = out_tx.send(OutboundMessage::ToClientArc {
                            client_id: inbound.client_id,
                            line,
                        });
                    }
                    ClientCommand::Place { .. } => {
                        let err = create_error(
                            inbound.seq,
                            ErrorCode::InvalidPlace,
                            "place not supported in acceptance harness",
                        );
                        let line: std::sync::Arc<str> =
                            std::sync::Arc::from(serde_json::to_string(&err).unwrap());
                        let _ = out_tx.send(OutboundMessage::ToClientArc {
                            client_id: inbound.client_id,
                            line,
                        });
                    }
                }

                // Always follow with an observation so acceptance checks can verify state.
                let last_event = gs
                    .take_last_event()
                    .map(tui_tetris::adapter::protocol::LastEvent::from);
                let snap = gs.snapshot();
                let obs = build_observation(obs_seq, &snap, last_event);
                obs_seq += 1;
                let line: std::sync::Arc<str> =
                    std::sync::Arc::from(serde_json::to_string(&obs).unwrap());
                let _ = out_tx.send(OutboundMessage::ToClientArc {
                    client_id: inbound.client_id,
                    line,
                });
            }
        }
    }
}

async fn engine_task_game_over(
    mut cmd_rx: mpsc::Receiver<InboundCommand>,
    out_tx: mpsc::UnboundedSender<OutboundMessage>,
) {
    enum Mode {
        GameOver,
        Playing(GameState),
    }

    let mut mode = Mode::GameOver;
    let mut snap = tui_tetris::core::GameSnapshot::default();
    snap.game_over = true;
    snap.paused = false;
    snap.seed = 1;
    snap.timers.drop_ms = 1000;
    let mut obs_seq: u64 = 100;

    while let Some(inbound) = cmd_rx.recv().await {
        match inbound.payload {
            InboundPayload::SnapshotRequest => {
                let (last_event, snap2) = match &mut mode {
                    Mode::GameOver => (None, snap),
                    Mode::Playing(gs) => (
                        gs.take_last_event()
                            .map(tui_tetris::adapter::protocol::LastEvent::from),
                        gs.snapshot(),
                    ),
                };
                let obs = build_observation(obs_seq, &snap2, last_event);
                obs_seq += 1;
                let line: std::sync::Arc<str> =
                    std::sync::Arc::from(serde_json::to_string(&obs).unwrap());
                let _ = out_tx.send(OutboundMessage::ToClientArc {
                    client_id: inbound.client_id,
                    line,
                });
            }
            InboundPayload::Command(cmd) => {
                match cmd {
                    ClientCommand::Actions(actions) => {
                        for a in actions {
                            match &mut mode {
                                Mode::GameOver => {
                                    if a == GameAction::Restart {
                                        let mut gs = GameState::new(1);
                                        gs.start();
                                        mode = Mode::Playing(gs);
                                    }
                                }
                                Mode::Playing(gs) => {
                                    let _ = gs.apply_action(a);
                                }
                            }
                        }

                        let ack = create_ack(inbound.seq, inbound.seq);
                        let line: std::sync::Arc<str> =
                            std::sync::Arc::from(serde_json::to_string(&ack).unwrap());
                        let _ = out_tx.send(OutboundMessage::ToClientArc {
                            client_id: inbound.client_id,
                            line,
                        });
                    }
                    ClientCommand::Place { .. } => {
                        let err = create_error(
                            inbound.seq,
                            ErrorCode::InvalidPlace,
                            "place not supported in acceptance harness",
                        );
                        let line: std::sync::Arc<str> =
                            std::sync::Arc::from(serde_json::to_string(&err).unwrap());
                        let _ = out_tx.send(OutboundMessage::ToClientArc {
                            client_id: inbound.client_id,
                            line,
                        });
                    }
                }

                // Always follow with an observation so acceptance checks can verify state.
                let (last_event, snap2) = match &mut mode {
                    Mode::GameOver => (None, snap),
                    Mode::Playing(gs) => (
                        gs.take_last_event()
                            .map(tui_tetris::adapter::protocol::LastEvent::from),
                        gs.snapshot(),
                    ),
                };
                let obs = build_observation(obs_seq, &snap2, last_event);
                obs_seq += 1;
                let line: std::sync::Arc<str> =
                    std::sync::Arc::from(serde_json::to_string(&obs).unwrap());
                let _ = out_tx.send(OutboundMessage::ToClientArc {
                    client_id: inbound.client_id,
                    line,
                });
            }
        }
    }
}

async fn broadcast_observations_task(out_tx: mpsc::UnboundedSender<OutboundMessage>) {
    let mut gs = GameState::new(1);
    gs.start();
    let mut seq: u64 = 10_000;

    loop {
        let snap = gs.snapshot();
        let obs = build_observation(seq, &snap, None);
        seq = seq.wrapping_add(1);
        let line: std::sync::Arc<str> =
            std::sync::Arc::from(serde_json::to_string(&obs).unwrap());
        let _ = out_tx.send(OutboundMessage::BroadcastArc {
            line,
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn acceptance_backpressure_does_not_stop_observations() {
    // Use a tiny inbound command channel and do not drain it.
    // The hello-triggered SnapshotRequest will fill the channel and subsequent commands
    // must return backpressure, while observations keep streaming.
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (server_handle, addr, _cmd_rx, out_tx) = spawn_server(config, 1).await;
    let obs_handle = tokio::spawn(broadcast_observations_task(out_tx));

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello
    let mut hello = create_hello(1, "acceptance", "2.0.0");
    hello.requested.stream_observations = true;
    hello.requested.command_mode = tui_tetris::adapter::protocol::CommandMode::Place;
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let welcome = read_json_line(&mut lines).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["seq"], 1);

    // Ensure observations are streaming before triggering backpressure.
    let mut saw_obs = false;
    for _ in 0..10 {
        let v = read_json_line(&mut lines).await;
        if v["type"] == "observation" {
            saw_obs = true;
            break;
        }
    }
    assert!(saw_obs);

    // Send a command. Since the inbound channel is full, expect backpressure.
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_half.write_all(cmd.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let mut saw_backpressure = false;
    let mut saw_obs_after_backpressure = false;
    for _ in 0..50 {
        let v = read_json_line(&mut lines).await;
        if !saw_backpressure {
            if v["type"] == "error" && v["seq"] == 2 && v["code"] == "backpressure" {
                saw_backpressure = true;
            }
            continue;
        }

        if v["type"] == "observation" {
            saw_obs_after_backpressure = true;
            break;
        }
    }

    assert!(saw_backpressure);
    assert!(saw_obs_after_backpressure);

    obs_handle.abort();
    server_handle.abort();
}

#[test]
fn acceptance_determinism_fixed_seed_reproduces_state_hash_sequence() {
    let seed = 12345;

    let mut a = GameState::new(seed);
    let mut b = GameState::new(seed);
    a.start();
    b.start();

    let mut hashes_a = Vec::new();
    let mut hashes_b = Vec::new();

    // Drive a deterministic sequence: hard-drop the current active piece each step.
    for i in 0..50u64 {
        if a.game_over() || b.game_over() || a.active().is_none() || b.active().is_none() {
            break;
        }

        assert!(a.apply_action(GameAction::HardDrop));
        assert!(b.apply_action(GameAction::HardDrop));

        // Consume line-clear pause so timers stay in sync.
        let _ = a.tick(1000, false);
        let _ = b.tick(1000, false);

        let last_a = a
            .take_last_event()
            .map(tui_tetris::adapter::protocol::LastEvent::from);
        let last_b = b
            .take_last_event()
            .map(tui_tetris::adapter::protocol::LastEvent::from);

        let snap_a = a.snapshot();
        let snap_b = b.snapshot();
        let obs_a = build_observation(i, &snap_a, last_a);
        let obs_b = build_observation(i, &snap_b, last_b);

        hashes_a.push(obs_a.state_hash);
        hashes_b.push(obs_b.state_hash);
    }

    assert!(!hashes_a.is_empty());
    assert_eq!(hashes_a, hashes_b);
}

#[tokio::test]
async fn acceptance_handshake_ordering_command_before_hello_returns_handshake_required() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let cmd = r#"{"type":"command","seq":1,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_half.write_all(cmd.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "handshake_required");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_handshake_ordering_control_before_hello_returns_handshake_required() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let ctrl = r#"{"type":"control","seq":1,"ts":1,"action":"claim"}"#;
    write_half.write_all(ctrl.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "handshake_required");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_protocol_mismatch_returns_error() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.stream_observations = false;
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "protocol_mismatch");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_control_enforces_monotonic_seq_after_hello() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello (seq must be 1)
    let mut hello = create_hello(1, "acceptance", "2.0.0");
    hello.requested.stream_observations = false;
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let welcome = read_json_line(&mut lines).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["seq"], 1);

    // release as controller (ok)
    let release = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    write_half.write_all(release.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let ack = read_json_line(&mut lines).await;
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["seq"], 2);

    // Duplicate seq must be rejected (strictly increasing).
    let release_dup = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    write_half.write_all(release_dup.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let err = read_json_line(&mut lines).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "invalid_command");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_parse_error_returns_invalid_command() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    write_half.write_all(b"{not json\n").await.unwrap();
    write_half.flush().await.unwrap();

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["code"], "invalid_command");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_observer_enforcement_not_controller() {
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
    let _ = read_json_line(&mut l1).await;

    // Client 2 (observer)
    let s2 = TcpStream::connect(addr).await.unwrap();
    let (r2, mut w2) = s2.into_split();
    let mut l2 = BufReader::new(r2).lines();
    let hello2 = create_hello(1, "c2", "2.0.0");
    w2.write_all(serde_json::to_string(&hello2).unwrap().as_bytes())
        .await
        .unwrap();
    w2.write_all(b"\n").await.unwrap();
    w2.flush().await.unwrap();
    let _ = read_json_line(&mut l2).await;

    // Observer tries to send a command.
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    w2.write_all(cmd.as_bytes()).await.unwrap();
    w2.write_all(b"\n").await.unwrap();
    w2.flush().await.unwrap();

    let v = read_json_line(&mut l2).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 2);
    assert_eq!(v["code"], "not_controller");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_ready_probe_welcome_then_playable_observation() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello requesting place + streaming observations
    let mut hello = create_hello(1, "acceptance", "2.0.0");
    hello.requested.stream_observations = true;
    hello.requested.command_mode = tui_tetris::adapter::protocol::CommandMode::Place;
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let welcome = read_json_line(&mut lines).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["seq"], 1);
    assert!(welcome.get("capabilities").is_some());

    let obs = read_json_line(&mut lines).await;
    assert_eq!(obs["type"], "observation");
    assert_eq!(obs["playable"], true);
    assert_eq!(obs["board"]["width"], 10);
    assert_eq!(obs["board"]["height"], 20);
    assert!(obs.get("active").is_some());
    assert!(obs.get("timers").is_some());

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_restart_and_pause_semantics() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(16);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello
    let hello = create_hello(1, "acceptance", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let _welcome = read_json_line(&mut lines).await;
    let _obs0 = read_json_line(&mut lines).await;

    // pause
    let cmd_pause = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["pause"]}"#;
    write_half.write_all(cmd_pause.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let ack = read_json_line(&mut lines).await;
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["seq"], 2);

    let obs = read_json_line(&mut lines).await;
    assert_eq!(obs["type"], "observation");
    assert_eq!(obs["paused"], true);
    assert_eq!(obs["playable"], false);

    // restart
    let cmd_restart = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["restart"]}"#;
    write_half.write_all(cmd_restart.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let ack2 = read_json_line(&mut lines).await;
    assert_eq!(ack2["type"], "ack");
    assert_eq!(ack2["seq"], 3);

    let obs2 = read_json_line(&mut lines).await;
    assert_eq!(obs2["type"], "observation");
    assert_eq!(obs2["paused"], false);
    assert_eq!(obs2["game_over"], false);
    assert_eq!(obs2["playable"], true);

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_actions_ignored_while_paused() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 16,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(16);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello
    let hello = create_hello(1, "acceptance", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let _welcome = read_json_line(&mut lines).await;
    let obs0 = read_json_line(&mut lines).await;
    assert_eq!(obs0["type"], "observation");
    assert_eq!(obs0["paused"], false);

    // pause
    let cmd_pause = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["pause"]}"#;
    write_half.write_all(cmd_pause.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let _ack_pause = read_json_line(&mut lines).await;
    let obs_paused = read_json_line(&mut lines).await;
    assert_eq!(obs_paused["type"], "observation");
    assert_eq!(obs_paused["paused"], true);

    let paused_hash = obs_paused["state_hash"].as_str().unwrap().to_string();
    let paused_piece_id = obs_paused["piece_id"].as_u64().unwrap();
    let paused_board_id = obs_paused["board_id"].as_u64().unwrap();
    let paused_active_x = obs_paused["active"]["x"].as_i64().unwrap();
    let paused_active_y = obs_paused["active"]["y"].as_i64().unwrap();

    // moveLeft while paused should be ignored (swiftui-tetris parity).
    let cmd_move = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_half.write_all(cmd_move.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let ack_move = read_json_line(&mut lines).await;
    assert_eq!(ack_move["type"], "ack");
    assert_eq!(ack_move["seq"], 3);

    let obs_after = read_json_line(&mut lines).await;
    assert_eq!(obs_after["type"], "observation");
    assert_eq!(obs_after["paused"], true);
    assert_eq!(obs_after["state_hash"].as_str().unwrap(), paused_hash);
    assert_eq!(obs_after["piece_id"].as_u64().unwrap(), paused_piece_id);
    assert_eq!(obs_after["board_id"].as_u64().unwrap(), paused_board_id);
    assert_eq!(obs_after["active"]["x"].as_i64().unwrap(), paused_active_x);
    assert_eq!(obs_after["active"]["y"].as_i64().unwrap(), paused_active_y);

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_actions_ignored_when_game_over() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 16,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(16);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_task_game_over(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello
    let hello = create_hello(1, "acceptance", "2.0.0");
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let _welcome = read_json_line(&mut lines).await;
    let obs0 = read_json_line(&mut lines).await;
    assert_eq!(obs0["type"], "observation");
    assert_eq!(obs0["paused"], false);
    assert_eq!(obs0["game_over"], true);
    assert_eq!(obs0["playable"], false);

    let game_over_hash = obs0["state_hash"].as_str().unwrap().to_string();
    let game_over_piece_id = obs0["piece_id"].as_u64().unwrap();
    let game_over_board_id = obs0["board_id"].as_u64().unwrap();
    assert!(obs0["active"].is_null());

    // moveLeft while game_over should be ignored (swiftui-tetris parity).
    let cmd_move = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_half.write_all(cmd_move.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let ack_move = read_json_line(&mut lines).await;
    assert_eq!(ack_move["type"], "ack");
    assert_eq!(ack_move["seq"], 2);

    let obs_after = read_json_line(&mut lines).await;
    assert_eq!(obs_after["type"], "observation");
    assert_eq!(obs_after["paused"], false);
    assert_eq!(obs_after["game_over"], true);
    assert_eq!(obs_after["playable"], false);
    assert_eq!(obs_after["state_hash"].as_str().unwrap(), game_over_hash);
    assert_eq!(obs_after["piece_id"].as_u64().unwrap(), game_over_piece_id);
    assert_eq!(obs_after["board_id"].as_u64().unwrap(), game_over_board_id);
    assert!(obs_after["active"].is_null());

    // restart should leave game_over and enter playable state.
    let cmd_restart = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["restart"]}"#;
    write_half.write_all(cmd_restart.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    let ack_restart = read_json_line(&mut lines).await;
    assert_eq!(ack_restart["type"], "ack");
    assert_eq!(ack_restart["seq"], 3);

    let obs_restart = read_json_line(&mut lines).await;
    assert_eq!(obs_restart["type"], "observation");
    assert_eq!(obs_restart["paused"], false);
    assert_eq!(obs_restart["game_over"], false);
    assert_eq!(obs_restart["playable"], true);
    assert!(!obs_restart["active"].is_null());

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_control_claim_release_and_controller_enforcement() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 16,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(16);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Client A (controller by default).
    let stream_a = TcpStream::connect(addr).await.unwrap();
    let (read_a, mut write_a) = stream_a.into_split();
    let mut lines_a = BufReader::new(read_a).lines();

    let hello_a = create_hello(1, "acceptance-a", "2.0.0");
    write_a
        .write_all(serde_json::to_string(&hello_a).unwrap().as_bytes())
        .await
        .unwrap();
    write_a.write_all(b"\n").await.unwrap();
    write_a.flush().await.unwrap();

    let welcome_a = read_json_line(&mut lines_a).await;
    assert_eq!(welcome_a["type"], "welcome");
    let obs_a0 = read_json_line(&mut lines_a).await;
    assert_eq!(obs_a0["type"], "observation");

    // Client B (observer).
    let stream_b = TcpStream::connect(addr).await.unwrap();
    let (read_b, mut write_b) = stream_b.into_split();
    let mut lines_b = BufReader::new(read_b).lines();

    let hello_b = create_hello(1, "acceptance-b", "2.0.0");
    write_b
        .write_all(serde_json::to_string(&hello_b).unwrap().as_bytes())
        .await
        .unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();

    let welcome_b = read_json_line(&mut lines_b).await;
    assert_eq!(welcome_b["type"], "welcome");
    let obs_b0 = read_json_line(&mut lines_b).await;
    assert_eq!(obs_b0["type"], "observation");

    // Observer cannot send commands.
    let cmd_b = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_b.write_all(cmd_b.as_bytes()).await.unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();

    let err_b = read_json_line(&mut lines_b).await;
    assert_eq!(err_b["type"], "error");
    assert_eq!(err_b["seq"], 2);
    assert_eq!(err_b["code"], "not_controller");

    // Observer cannot claim while controller is active.
    let claim_b = r#"{"type":"control","seq":3,"ts":1,"action":"claim"}"#;
    write_b.write_all(claim_b.as_bytes()).await.unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();

    let err_claim = read_json_line(&mut lines_b).await;
    assert_eq!(err_claim["type"], "error");
    assert_eq!(err_claim["seq"], 3);
    assert_eq!(err_claim["code"], "controller_active");

    // Controller releases.
    let release_a = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    write_a.write_all(release_a.as_bytes()).await.unwrap();
    write_a.write_all(b"\n").await.unwrap();
    write_a.flush().await.unwrap();

    let ack_release = read_json_line(&mut lines_a).await;
    assert_eq!(ack_release["type"], "ack");
    assert_eq!(ack_release["seq"], 2);

    // Observer can claim now.
    let claim_b2 = r#"{"type":"control","seq":4,"ts":1,"action":"claim"}"#;
    write_b.write_all(claim_b2.as_bytes()).await.unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();

    let ack_claim = read_json_line(&mut lines_b).await;
    assert_eq!(ack_claim["type"], "ack");
    assert_eq!(ack_claim["seq"], 4);

    // New controller can send a command (ack comes from the engine task).
    let cmd_b2 = r#"{"type":"command","seq":5,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_b.write_all(cmd_b2.as_bytes()).await.unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();

    let ack_cmd = read_json_line(&mut lines_b).await;
    assert_eq!(ack_cmd["type"], "ack");
    assert_eq!(ack_cmd["seq"], 5);
    let obs_b1 = read_json_line(&mut lines_b).await;
    assert_eq!(obs_b1["type"], "observation");

    // Old controller cannot send commands anymore.
    let cmd_a = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_a.write_all(cmd_a.as_bytes()).await.unwrap();
    write_a.write_all(b"\n").await.unwrap();
    write_a.flush().await.unwrap();

    let err_a = read_json_line(&mut lines_a).await;
    assert_eq!(err_a["type"], "error");
    assert_eq!(err_a["seq"], 3);
    assert_eq!(err_a["code"], "not_controller");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_control_release_requires_controller() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 16,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(16);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Client A (controller by default).
    let stream_a = TcpStream::connect(addr).await.unwrap();
    let (read_a, mut write_a) = stream_a.into_split();
    let mut lines_a = BufReader::new(read_a).lines();

    let hello_a = create_hello(1, "acceptance-a", "2.0.0");
    write_a
        .write_all(serde_json::to_string(&hello_a).unwrap().as_bytes())
        .await
        .unwrap();
    write_a.write_all(b"\n").await.unwrap();
    write_a.flush().await.unwrap();
    let _welcome_a = read_json_line(&mut lines_a).await;
    let _obs_a0 = read_json_line(&mut lines_a).await;

    // Client B (observer).
    let stream_b = TcpStream::connect(addr).await.unwrap();
    let (read_b, mut write_b) = stream_b.into_split();
    let mut lines_b = BufReader::new(read_b).lines();

    let hello_b = create_hello(1, "acceptance-b", "2.0.0");
    write_b
        .write_all(serde_json::to_string(&hello_b).unwrap().as_bytes())
        .await
        .unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();
    let _welcome_b = read_json_line(&mut lines_b).await;
    let _obs_b0 = read_json_line(&mut lines_b).await;

    // Non-controller release must be rejected.
    let release_b = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    write_b.write_all(release_b.as_bytes()).await.unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();

    let err = read_json_line(&mut lines_b).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "not_controller");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_controller_disconnect_promotes_next_client() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 16,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(16);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Client A (controller by default).
    let stream_a = TcpStream::connect(addr).await.unwrap();
    let (read_a, mut write_a) = stream_a.into_split();
    let mut lines_a = BufReader::new(read_a).lines();

    let hello_a = create_hello(1, "acceptance-a", "2.0.0");
    write_a
        .write_all(serde_json::to_string(&hello_a).unwrap().as_bytes())
        .await
        .unwrap();
    write_a.write_all(b"\n").await.unwrap();
    write_a.flush().await.unwrap();

    let welcome_a = read_json_line(&mut lines_a).await;
    assert_eq!(welcome_a["type"], "welcome");
    let _obs_a0 = read_json_line(&mut lines_a).await;

    // Client B (observer initially).
    let stream_b = TcpStream::connect(addr).await.unwrap();
    let (read_b, mut write_b) = stream_b.into_split();
    let mut lines_b = BufReader::new(read_b).lines();

    let hello_b = create_hello(1, "acceptance-b", "2.0.0");
    write_b
        .write_all(serde_json::to_string(&hello_b).unwrap().as_bytes())
        .await
        .unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();

    let welcome_b = read_json_line(&mut lines_b).await;
    assert_eq!(welcome_b["type"], "welcome");
    let _obs_b0 = read_json_line(&mut lines_b).await;

    // Disconnect controller A.
    drop(write_a);
    drop(lines_a);

    // B should be promoted to controller and commands should succeed.
    let cmd_b = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    write_b.write_all(cmd_b.as_bytes()).await.unwrap();
    write_b.write_all(b"\n").await.unwrap();
    write_b.flush().await.unwrap();

    // We may see an observation before the ack depending on scheduling; scan a few frames.
    let mut saw_ack = false;
    for _ in 0..10 {
        let v = read_json_line(&mut lines_b).await;
        if v["type"] == "ack" && v["seq"] == 2 {
            saw_ack = true;
            break;
        }
    }
    assert!(saw_ack, "expected ack after controller disconnect promotion");

    server_handle.abort();
    engine_handle.abort();
}
