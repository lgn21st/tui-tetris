use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::protocol::{create_ack, create_error, create_hello, ErrorCode, LastEvent};
use tui_tetris::adapter::runtime::InboundPayload;
use tui_tetris::adapter::server::{build_observation, run_server, ServerConfig};
use tui_tetris::adapter::{ClientCommand, InboundCommand, OutboundMessage};
use tui_tetris::core::{get_shape, GameState};
use tui_tetris::engine::place::apply_place;
use tui_tetris::types::{PieceKind, Rotation};

async fn read_line(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
) -> String {
    tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .expect("timeout waiting for line")
        .expect("io error")
        .expect("expected line")
}

async fn read_next_json(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
) -> serde_json::Value {
    serde_json::from_str(&read_line(lines).await).expect("valid json line")
}

fn last_event_from_core(gs: &mut GameState) -> Option<LastEvent> {
    gs.take_last_event().map(LastEvent::from)
}

async fn engine_loop(
    mut cmd_rx: mpsc::Receiver<InboundCommand>,
    out_tx: mpsc::UnboundedSender<OutboundMessage>,
) {
    let mut gs = GameState::new(1);
    gs.start();

    while let Some(inbound) = cmd_rx.recv().await {
        match inbound.payload {
            InboundPayload::SnapshotRequest => {
                let last_event = last_event_from_core(&mut gs);
                let snap = gs.snapshot();
                let obs = build_observation(inbound.seq, &snap, last_event);
                let _ = out_tx.send(OutboundMessage::ToClientArc {
                    client_id: inbound.client_id,
                    line: std::sync::Arc::from(serde_json::to_string(&obs).unwrap()),
                });
            }
            InboundPayload::Command(cmd) => {
                let result = match cmd {
                    ClientCommand::Actions {
                        actions,
                        mut restart_seed,
                    } => {
                        for a in actions {
                            if a == tui_tetris::types::GameAction::Restart {
                                if let Some(seed) = restart_seed.take() {
                                    let _ = gs.restart_with_seed(seed);
                                    continue;
                                }
                            }
                            let _ = gs.apply_action(a);
                        }
                        Ok(())
                    }
                    ClientCommand::Place {
                        x,
                        rotation,
                        use_hold,
                    } => apply_place(&mut gs, x, rotation, use_hold).map(|_| ()),
                };

                // Let timers advance so we don't get stuck in line-clear pause.
                let _ = gs.tick(1000, false);

                // ack/error
                match result {
                    Ok(()) => {
                        let ack = create_ack(inbound.seq, inbound.seq);
                        let _ = out_tx.send(OutboundMessage::ToClientArc {
                            client_id: inbound.client_id,
                            line: std::sync::Arc::from(serde_json::to_string(&ack).unwrap()),
                        });
                    }
                    Err(e) => {
                        let code = match e.code() {
                            "hold_unavailable" => ErrorCode::HoldUnavailable,
                            "invalid_place" => ErrorCode::InvalidPlace,
                            _ => ErrorCode::InvalidCommand,
                        };
                        let err = create_error(inbound.seq, code, e.message());
                        let _ = out_tx.send(OutboundMessage::ToClientArc {
                            client_id: inbound.client_id,
                            line: std::sync::Arc::from(serde_json::to_string(&err).unwrap()),
                        });
                    }
                }

                // follow with an observation
                let last_event = last_event_from_core(&mut gs);
                let snap = gs.snapshot();
                let obs = build_observation(inbound.seq.wrapping_add(10_000), &snap, last_event);
                let _ = out_tx.send(OutboundMessage::ToClientArc {
                    client_id: inbound.client_id,
                    line: std::sync::Arc::from(serde_json::to_string(&obs).unwrap()),
                });
            }
        }
    }
}

fn compute_leftmost_x(kind: PieceKind, rot: Rotation) -> i8 {
    let shape = get_shape(kind, rot);
    let mut min_dx = i8::MAX;
    for (dx, _) in shape {
        min_dx = min_dx.min(dx);
    }
    -min_dx
}

async fn collect_next_queue_signature(addr: std::net::SocketAddr, seed: u32) -> Vec<Vec<String>> {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let mut seq: u64 = 1;
    let mut hello = create_hello(seq, "restart-seed", "2.0.0");
    hello.requested.stream_observations = true;
    hello.requested.command_mode = tui_tetris::adapter::protocol::CommandMode::Action;
    hello.requested.role = Some(tui_tetris::adapter::protocol::RequestedRole::Controller);
    write_half
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    // welcome + first observation (snapshot request)
    let welcome = read_next_json(&mut lines).await;
    assert_eq!(welcome["type"], "welcome");
    let first_obs = read_next_json(&mut lines).await;
    assert_eq!(first_obs["type"], "observation");
    let baseline_episode_id = first_obs["episode_id"].as_u64().unwrap_or(0);

    // claim controller (idempotent if already controller)
    seq += 1;
    let claim = serde_json::json!({"type":"control","seq":seq,"ts":1,"action":"claim"});
    write_half
        .write_all(serde_json::to_string(&claim).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
    let claim_resp = read_next_json(&mut lines).await;
    assert!(claim_resp["type"] == "ack" || claim_resp["type"] == "error");

    // restart with seed
    seq += 1;
    let restart = serde_json::json!({
        "type":"command",
        "seq":seq,
        "ts":1,
        "mode":"action",
        "actions":["restart"],
        "restart":{"seed":seed}
    });
    write_half
        .write_all(serde_json::to_string(&restart).unwrap().as_bytes())
        .await
        .unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();

    // ack + observation after restart
    loop {
        let v = read_next_json(&mut lines).await;
        if v["type"] == "ack" {
            assert_eq!(v["seq"], seq);
            break;
        }
        if v["type"] == "error" {
            panic!("restart error: {v}");
        }
    }
    // Depending on timing, the adapter may emit one or more observations that
    // were in flight from the pre-restart episode. Wait for the first observation
    // of the new episode (episode_id changed) and the first step of the new piece.
    loop {
        let obs = read_next_json(&mut lines).await;
        if obs["type"] != "observation" {
            continue;
        }
        let ep = obs["episode_id"].as_u64().unwrap_or(0);
        let step = obs["step_in_piece"].as_u64().unwrap_or(0);
        let got_seed = obs["seed"].as_u64().unwrap_or(0) as u32;
        if ep != baseline_episode_id && step == 1 && got_seed == seed {
            break;
        }
    }

    let mut sig: Vec<Vec<String>> = Vec::new();

    // Capture a lightweight signature: the next_queue after each hard drop.
    for _ in 0..8 {
        seq += 1;
        let cmd = serde_json::json!({
            "type":"command",
            "seq":seq,
            "ts":1,
            "mode":"action",
            "actions":["hardDrop"]
        });
        write_half
            .write_all(serde_json::to_string(&cmd).unwrap().as_bytes())
            .await
            .unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        // ack (or error if game isn't playable)
        loop {
            let v = read_next_json(&mut lines).await;
            if v["type"] == "ack" || v["type"] == "error" {
                assert_eq!(v["seq"], seq);
                if v["type"] == "error" {
                    panic!("hardDrop error: {v}");
                }
                break;
            }
        }

        let obs = read_next_json(&mut lines).await;
        assert_eq!(obs["type"], "observation");
        assert_eq!(obs["seed"], seed);
        let q = obs["next_queue"]
            .as_array()
            .expect("next_queue array")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        sig.push(q);
    }

    sig
}

#[tokio::test]
async fn closed_loop_stability_3x50_reconnects() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 64,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(128);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_loop(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // 3 runs, 50 episodes each; reconnect every episode.
    for _run in 0..3 {
        for _episode in 0..50 {
            let stream = TcpStream::connect(addr).await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut lines = BufReader::new(read_half).lines();

            let mut seq: u64 = 1;
            let mut hello = create_hello(seq, "closed-loop", "2.0.0");
            hello.requested.stream_observations = true;
            hello.requested.command_mode = tui_tetris::adapter::protocol::CommandMode::Place;
            write_half
                .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
                .await
                .unwrap();
            write_half.write_all(b"\n").await.unwrap();
            write_half.flush().await.unwrap();

            // welcome
            let welcome: serde_json::Value = serde_json::from_str(&read_line(&mut lines).await).unwrap();
            assert_eq!(welcome["type"], "welcome");

            // first observation (from snapshot request)
            let mut obs: serde_json::Value = serde_json::from_str(&read_line(&mut lines).await).unwrap();
            assert_eq!(obs["type"], "observation");

            // Drive until game over or safety cap.
            let mut placements = 0u32;
            while obs["game_over"] != true && placements < 300 {
                let active = obs.get("active").and_then(|v| v.as_object()).cloned();
                if active.is_none() {
                    break;
                }

                let kind_s = active.as_ref().unwrap().get("kind").and_then(|v| v.as_str()).unwrap();
                let rot_s = active.as_ref().unwrap().get("rotation").and_then(|v| v.as_str()).unwrap();
                let kind = PieceKind::from_str(kind_s).expect("piece kind");
                let rot = Rotation::from_str(rot_s).expect("rotation");
                let x = compute_leftmost_x(kind, rot);

                seq += 1;
                let cmd = serde_json::json!({
                    "type": "command",
                    "seq": seq,
                    "ts": 1,
                    "mode": "place",
                    "place": {"x": x, "rotation": rot.as_str(), "useHold": false}
                });
                write_half
                    .write_all(serde_json::to_string(&cmd).unwrap().as_bytes())
                    .await
                    .unwrap();
                write_half.write_all(b"\n").await.unwrap();
                write_half.flush().await.unwrap();

                // Expect ack or error for this seq.
                loop {
                    let v: serde_json::Value = serde_json::from_str(&read_line(&mut lines).await).unwrap();
                    if v["type"] == "ack" || v["type"] == "error" {
                        assert_eq!(v["seq"], seq);
                        break;
                    }
                }

                // Next line should be an observation.
                obs = serde_json::from_str(&read_line(&mut lines).await).unwrap();
                assert_eq!(obs["type"], "observation");
                placements += 1;
            }

            // End episode.
            drop(write_half);
        }
    }

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn restart_with_seed_is_deterministic_for_next_queue() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 64,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(128);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_loop(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    let seed = 123u32;
    let a = collect_next_queue_signature(addr, seed).await;
    let b = collect_next_queue_signature(addr, seed).await;
    assert_eq!(a, b);

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
#[ignore]
async fn closed_loop_long_run_200_episodes() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 64,
        log_path: None,
        ..ServerConfig::default()
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(256);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), None).await;
    });
    let engine_handle = tokio::spawn(engine_loop(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    for _episode in 0..200 {
        let stream = TcpStream::connect(addr).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let mut seq: u64 = 1;
        let mut hello = create_hello(seq, "closed-loop-long", "2.0.0");
        hello.requested.stream_observations = true;
        hello.requested.command_mode = tui_tetris::adapter::protocol::CommandMode::Place;
        write_half
            .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
            .await
            .unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        let welcome: serde_json::Value =
            serde_json::from_str(&read_line(&mut lines).await).unwrap();
        assert_eq!(welcome["type"], "welcome");

        // First observation (from snapshot request)
        let mut obs: serde_json::Value =
            serde_json::from_str(&read_line(&mut lines).await).unwrap();
        assert_eq!(obs["type"], "observation");

        // Restart each episode to keep the loop playable even after game-over.
        seq += 1;
        let restart = serde_json::json!({
            "type": "command",
            "seq": seq,
            "ts": 1,
            "mode": "action",
            "actions": ["restart"]
        });
        write_half
            .write_all(serde_json::to_string(&restart).unwrap().as_bytes())
            .await
            .unwrap();
        write_half.write_all(b"\n").await.unwrap();
        write_half.flush().await.unwrap();

        // Expect ack for restart.
        loop {
            let v: serde_json::Value =
                serde_json::from_str(&read_line(&mut lines).await).unwrap();
            if v["type"] == "ack" {
                assert_eq!(v["seq"], seq);
                break;
            }
        }

        // Observation after restart.
        obs = serde_json::from_str(&read_line(&mut lines).await).unwrap();
        assert_eq!(obs["type"], "observation");
        assert_eq!(obs["playable"], true);

        // Drive a bounded number of placements.
        let mut placements = 0u32;
        while obs["game_over"] != true && placements < 100 {
            let active = obs.get("active").and_then(|v| v.as_object()).cloned();
            if active.is_none() {
                break;
            }

            let kind_s = active
                .as_ref()
                .unwrap()
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap();
            let rot_s = active
                .as_ref()
                .unwrap()
                .get("rotation")
                .and_then(|v| v.as_str())
                .unwrap();
            let kind = PieceKind::from_str(kind_s).expect("piece kind");
            let rot = Rotation::from_str(rot_s).expect("rotation");
            let x = compute_leftmost_x(kind, rot);

            seq += 1;
            let cmd = serde_json::json!({
                "type": "command",
                "seq": seq,
                "ts": 1,
                "mode": "place",
                "place": {"x": x, "rotation": rot.as_str(), "useHold": false}
            });
            write_half
                .write_all(serde_json::to_string(&cmd).unwrap().as_bytes())
                .await
                .unwrap();
            write_half.write_all(b"\n").await.unwrap();
            write_half.flush().await.unwrap();

            // Expect ack or error for this seq.
            loop {
                let v: serde_json::Value =
                    serde_json::from_str(&read_line(&mut lines).await).unwrap();
                if v["type"] == "ack" || v["type"] == "error" {
                    assert_eq!(v["seq"], seq);
                    break;
                }
            }

            obs = serde_json::from_str(&read_line(&mut lines).await).unwrap();
            assert_eq!(obs["type"], "observation");
            placements += 1;
        }

        drop(write_half);
    }

    server_handle.abort();
    engine_handle.abort();
}
