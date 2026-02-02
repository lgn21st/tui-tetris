use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use tui_tetris::adapter::protocol::{
    create_ack, create_error, create_hello, ErrorCode, LastEvent, TSpinLower,
};
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

fn last_event_from_core(gs: &mut GameState) -> Option<LastEvent> {
    gs.take_last_event().map(|ev| LastEvent {
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
    })
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
                let _ = out_tx.send(OutboundMessage::ToClient {
                    client_id: inbound.client_id,
                    line: serde_json::to_string(&obs).unwrap(),
                });
            }
            InboundPayload::Command(cmd) => {
                let result = match cmd {
                    ClientCommand::Actions(actions) => {
                        for a in actions {
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
                        let _ = out_tx.send(OutboundMessage::ToClient {
                            client_id: inbound.client_id,
                            line: serde_json::to_string(&ack).unwrap(),
                        });
                    }
                    Err(e) => {
                        let code = match e.code() {
                            "hold_unavailable" => ErrorCode::HoldUnavailable,
                            "invalid_place" => ErrorCode::InvalidPlace,
                            _ => ErrorCode::InvalidCommand,
                        };
                        let err = create_error(inbound.seq, code, e.message());
                        let _ = out_tx.send(OutboundMessage::ToClient {
                            client_id: inbound.client_id,
                            line: serde_json::to_string(&err).unwrap(),
                        });
                    }
                }

                // follow with an observation
                let last_event = last_event_from_core(&mut gs);
                let snap = gs.snapshot();
                let obs = build_observation(inbound.seq.wrapping_add(10_000), &snap, last_event);
                let _ = out_tx.send(OutboundMessage::ToClient {
                    client_id: inbound.client_id,
                    line: serde_json::to_string(&obs).unwrap(),
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

#[tokio::test]
async fn closed_loop_stability_3x50_reconnects() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 64,
        log_path: None,
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(128);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
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
#[ignore]
async fn closed_loop_long_run_200_episodes() {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        protocol_version: "2.0.0".to_string(),
        max_pending_commands: 64,
        log_path: None,
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(256);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx)).await;
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
