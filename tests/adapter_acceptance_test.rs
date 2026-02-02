use std::time::Duration;
use std::net::SocketAddr;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::protocol::{create_ack, create_error, create_hello, ErrorCode, TSpinLower};
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
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
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
                let last_event = gs.take_last_event().map(|ev| tui_tetris::adapter::protocol::LastEvent {
                    locked: ev.locked,
                    lines_cleared: ev.lines_cleared,
                    line_clear_score: ev.line_clear_score,
                    tspin: ev.tspin.and_then(|t| match t {
                        tui_tetris::types::TSpinKind::Mini => Some(TSpinLower::Mini),
                        tui_tetris::types::TSpinKind::Full => Some(TSpinLower::Full),
                        tui_tetris::types::TSpinKind::None => None,
                    }),
                    combo: ev.combo,
                    back_to_back: ev.back_to_back,
                });
                let snap = gs.snapshot();
                let obs = build_observation(obs_seq, &snap, last_event);
                obs_seq += 1;
                let _ = out_tx.send(OutboundMessage::ToClient {
                    client_id: inbound.client_id,
                    line: serde_json::to_string(&obs).unwrap(),
                });
            }
            InboundPayload::Command(cmd) => {
                match cmd {
                    ClientCommand::Actions(actions) => {
                        for a in actions {
                            let _ = gs.apply_action(a);
                        }
                        let ack = create_ack(inbound.seq, inbound.seq);
                        let _ = out_tx.send(OutboundMessage::ToClient {
                            client_id: inbound.client_id,
                            line: serde_json::to_string(&ack).unwrap(),
                        });
                    }
                    ClientCommand::Place { .. } => {
                        let err = create_error(
                            inbound.seq,
                            ErrorCode::InvalidPlace,
                            "place not supported in acceptance harness",
                        );
                        let _ = out_tx.send(OutboundMessage::ToClient {
                            client_id: inbound.client_id,
                            line: serde_json::to_string(&err).unwrap(),
                        });
                    }
                }

                // Always follow with an observation so acceptance checks can verify state.
                let last_event = gs.take_last_event().map(|ev| tui_tetris::adapter::protocol::LastEvent {
                    locked: ev.locked,
                    lines_cleared: ev.lines_cleared,
                    line_clear_score: ev.line_clear_score,
                    tspin: ev.tspin.and_then(|t| match t {
                        tui_tetris::types::TSpinKind::Mini => Some(TSpinLower::Mini),
                        tui_tetris::types::TSpinKind::Full => Some(TSpinLower::Full),
                        tui_tetris::types::TSpinKind::None => None,
                    }),
                    combo: ev.combo,
                    back_to_back: ev.back_to_back,
                });
                let snap = gs.snapshot();
                let obs = build_observation(obs_seq, &snap, last_event);
                obs_seq += 1;
                let _ = out_tx.send(OutboundMessage::ToClient {
                    client_id: inbound.client_id,
                    line: serde_json::to_string(&obs).unwrap(),
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
        let _ = out_tx.send(OutboundMessage::Broadcast {
            line: serde_json::to_string(&obs).unwrap(),
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
    };

    let (server_handle, addr, _cmd_rx, out_tx) = spawn_server(config, 1).await;
    let obs_handle = tokio::spawn(broadcast_observations_task(out_tx));

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    // hello
    let mut hello = create_hello(1, "acceptance", "2.0.0");
    hello.requested.stream_observations = true;
    hello.requested.command_mode = "place".to_string();
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
        if a.game_over || b.game_over || a.active.is_none() || b.active.is_none() {
            break;
        }

        assert!(a.apply_action(GameAction::HardDrop));
        assert!(b.apply_action(GameAction::HardDrop));

        // Consume line-clear pause so timers stay in sync.
        let _ = a.tick(1000, false);
        let _ = b.tick(1000, false);

        let last_a = a
            .take_last_event()
            .map(|ev| tui_tetris::adapter::protocol::LastEvent {
                locked: ev.locked,
                lines_cleared: ev.lines_cleared,
                line_clear_score: ev.line_clear_score,
                tspin: ev.tspin.and_then(|t| match t {
                    tui_tetris::types::TSpinKind::Mini => Some(TSpinLower::Mini),
                    tui_tetris::types::TSpinKind::Full => Some(TSpinLower::Full),
                    tui_tetris::types::TSpinKind::None => None,
                }),
                combo: ev.combo,
                back_to_back: ev.back_to_back,
            });
        let last_b = b
            .take_last_event()
            .map(|ev| tui_tetris::adapter::protocol::LastEvent {
                locked: ev.locked,
                lines_cleared: ev.lines_cleared,
                line_clear_score: ev.line_clear_score,
                tspin: ev.tspin.and_then(|t| match t {
                    tui_tetris::types::TSpinKind::Mini => Some(TSpinLower::Mini),
                    tui_tetris::types::TSpinKind::Full => Some(TSpinLower::Full),
                    tui_tetris::types::TSpinKind::None => None,
                }),
                combo: ev.combo,
                back_to_back: ev.back_to_back,
            });

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
async fn acceptance_protocol_mismatch_returns_error() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
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
async fn acceptance_parse_error_returns_invalid_command() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 8,
        log_path: None,
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
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
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
    hello.requested.command_mode = "place".to_string();
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
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(16);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
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
