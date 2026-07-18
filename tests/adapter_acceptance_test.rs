#![allow(clippy::field_reassign_with_default)] // Stepwise fixtures mirror protocol phases.
#![allow(clippy::large_enum_variant)] // Test-only state machines favor direct GameState access.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};

use tetris_adapter::adapter::server::{build_observation, run_server};
use tetris_adapter::adapter::{InboundCommand, OutboundMessage};
use tetris_adapter_protocol::protocol::create_hello;
use tetris_core::core::GameState;
use tetris_core::types::GameAction;

mod support;
use support::{read_json_line, spawn_server};

async fn engine_task(
    mut cmd_rx: mpsc::Receiver<InboundCommand>,
    _out_tx: mpsc::UnboundedSender<OutboundMessage>,
) {
    let mut driver = tetris_adapter::adapter::game_loop::SessionProtocolDriver::new(1, 20);
    while let Some(inbound) = cmd_rx.recv().await {
        driver.handle(inbound);
    }
}

fn game_over_driver() -> tetris_adapter::adapter::game_loop::SessionProtocolDriver {
    let mut game = GameState::new(1);
    game.start();
    for _ in 0..5 {
        let _ = game.apply_action(GameAction::SoftDrop);
    }
    for y in 0..4i8 {
        for x in 1..tetris_core::types::BOARD_WIDTH as i8 {
            let _ = game
                .board_mut()
                .set(x, y, Some(tetris_core::types::PieceKind::I));
        }
    }
    let _ = game.apply_action(GameAction::HardDrop);
    assert!(game.game_over());
    let _ = game.take_last_event();
    let session = tetris_session::engine::session::SessionRuntime::from_game(game);
    tetris_adapter::adapter::game_loop::SessionProtocolDriver::from_session(session, 20)
}

async fn engine_task_game_over(
    mut cmd_rx: mpsc::Receiver<InboundCommand>,
    _out_tx: mpsc::UnboundedSender<OutboundMessage>,
) {
    let mut driver = game_over_driver();
    while let Some(inbound) = cmd_rx.recv().await {
        driver.handle(inbound);
    }
}

async fn broadcast_observations_task(out_tx: mpsc::UnboundedSender<OutboundMessage>) {
    let mut gs = GameState::new(1);
    gs.start();
    let mut seq: u64 = 10_000;

    loop {
        let snap = gs.snapshot();
        let obs = build_observation(seq, seq, &snap, &[]);
        seq = seq.wrapping_add(1);
        let _ = out_tx.send(OutboundMessage::BroadcastObservationArc {
            obs: std::sync::Arc::new(obs),
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn acceptance_backpressure_does_not_stop_observations() {
    // Use a tiny inbound command channel and do not drain it.
    // The hello-triggered SnapshotRequest will fill the channel and subsequent commands
    // must return backpressure, while observations keep streaming.
    let config = support::server_config();

    let (server_handle, addr, _cmd_rx, out_tx) = spawn_server(config, 1).await;
    let obs_handle = tokio::spawn(broadcast_observations_task(out_tx));

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.stream_observations = true;
    hello.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    support::write_json_line(&mut write_half, &hello).await;

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
    support::write_raw_line(&mut write_half, cmd).await;

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
            .map(tetris_adapter_protocol::protocol::TransitionEvent::from);
        let last_b = b
            .take_last_event()
            .map(tetris_adapter_protocol::protocol::TransitionEvent::from);

        let snap_a = a.snapshot();
        let snap_b = b.snapshot();
        let events_a = last_a.into_iter().collect::<Vec<_>>();
        let events_b = last_b.into_iter().collect::<Vec<_>>();
        let obs_a = build_observation(i, i, &snap_a, &events_a);
        let obs_b = build_observation(i, i, &snap_b, &events_b);

        hashes_a.push(obs_a.state_hash);
        hashes_b.push(obs_b.state_hash);
    }

    assert!(!hashes_a.is_empty());
    assert_eq!(hashes_a, hashes_b);
}

#[tokio::test]
async fn acceptance_handshake_ordering_command_before_hello_returns_handshake_required() {
    let config = support::server_config();

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let (mut lines, mut write_half) = support::connect(addr).await;

    let cmd = r#"{"type":"command","seq":1,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_half, cmd).await;

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "handshake_required");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_handshake_ordering_control_before_hello_returns_handshake_required() {
    let config = support::server_config();

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let (mut lines, mut write_half) = support::connect(addr).await;

    let ctrl = r#"{"type":"control","seq":1,"ts":1,"action":"claim"}"#;
    support::write_raw_line(&mut write_half, ctrl).await;

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "handshake_required");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_hello_seq_must_be_one_and_does_not_handshake() {
    let config = support::server_config();

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello with seq!=1 must be rejected and MUST NOT handshake the connection.
    let hello = r#"{"type":"hello","seq":2,"ts":1,"client":{"name":"acceptance","version":"0.1.0"},"protocol_version":"3.0.0","formats":["json"],"requested":{"stream_observations":false,"command_mode":"place"}}"#;
    support::write_raw_line(&mut write_half, hello).await;

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 2);
    assert_eq!(v["code"], "invalid_command");

    // Since hello was rejected, command should still require handshake.
    let cmd = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_half, cmd).await;

    let v2 = read_json_line(&mut lines).await;
    assert_eq!(v2["type"], "error");
    assert_eq!(v2["seq"], 3);
    assert_eq!(v2["code"], "handshake_required");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_hello_formats_must_include_json_and_does_not_handshake() {
    let config = support::server_config();

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello without json format must be rejected and MUST NOT handshake the connection.
    let hello = r#"{"type":"hello","seq":1,"ts":1,"client":{"name":"acceptance","version":"0.1.0"},"protocol_version":"3.0.0","formats":["text"],"requested":{"stream_observations":false,"command_mode":"place"}}"#;
    support::write_raw_line(&mut write_half, hello).await;

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "invalid_command");

    // Since hello was rejected, command should still require handshake.
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_half, cmd).await;

    let v2 = read_json_line(&mut lines).await;
    assert_eq!(v2["type"], "error");
    assert_eq!(v2["seq"], 2);
    assert_eq!(v2["code"], "handshake_required");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_place_x_out_of_bounds_returns_invalid_place() {
    let config = support::server_config_with_capacity(16);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello (request place mode)
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    support::write_json_line(&mut write_half, &hello).await;

    let _welcome = read_json_line(&mut lines).await;
    let obs0 = read_json_line(&mut lines).await;
    assert_eq!(obs0["type"], "observation");
    let rot0 = obs0["active"]["rotation"].as_str().unwrap();

    // place with x out of bounds
    let cmd = format!(
        "{{\"type\":\"command\",\"seq\":2,\"ts\":1,\"mode\":\"place\",\"place\":{{\"x\":-50,\"rotation\":\"{}\",\"useHold\":false}}}}",
        rot0
    );
    support::write_raw_line(&mut write_half, cmd).await;

    let err = read_json_line(&mut lines).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "invalid_place");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_place_use_hold_when_unavailable_returns_hold_unavailable() {
    let config = support::server_config_with_capacity(16);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello (request place mode)
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    support::write_json_line(&mut write_half, &hello).await;

    let _welcome = read_json_line(&mut lines).await;
    let obs0 = read_json_line(&mut lines).await;
    assert_eq!(obs0["type"], "observation");
    let x0 = obs0["active"]["x"].as_i64().unwrap();
    let rot0 = obs0["active"]["rotation"].as_str().unwrap();

    // Use hold via action mode first (consumes can_hold).
    let cmd_hold = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["hold"]}"#;
    support::write_raw_line(&mut write_half, cmd_hold).await;
    let ack = read_json_line(&mut lines).await;
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["seq"], 2);
    let _obs1 = read_json_line(&mut lines).await;

    // Now request useHold again in place command; should fail with hold_unavailable.
    let cmd_place = format!(
        "{{\"type\":\"command\",\"seq\":3,\"ts\":1,\"mode\":\"place\",\"place\":{{\"x\":{},\"rotation\":\"{}\",\"useHold\":true}}}}",
        x0, rot0
    );
    support::write_raw_line(&mut write_half, cmd_place).await;

    let err = read_json_line(&mut lines).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 3);
    assert_eq!(err["code"], "hold_unavailable");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_protocol_mismatch_returns_error() {
    let config = support::server_config();

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let (mut lines, mut write_half) = support::connect(addr).await;

    let mut hello = create_hello(1, "acceptance", "2.1.1");
    hello.requested.stream_observations = false;
    support::write_json_line(&mut write_half, &hello).await;

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 1);
    assert_eq!(v["code"], "protocol_mismatch");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_control_enforces_monotonic_seq_after_hello() {
    let config = support::server_config();

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello (seq must be 1)
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.stream_observations = false;
    support::write_json_line(&mut write_half, &hello).await;

    let welcome = read_json_line(&mut lines).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["seq"], 1);

    // release as controller (ok)
    let release = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    support::write_raw_line(&mut write_half, release).await;

    let ack = read_json_line(&mut lines).await;
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["seq"], 2);

    // Duplicate seq must be rejected (strictly increasing).
    let release_dup = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    support::write_raw_line(&mut write_half, release_dup).await;

    let err = read_json_line(&mut lines).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "invalid_command");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_command_enforces_monotonic_seq_after_hello() {
    let config = support::server_config_with_capacity(16);

    let (server_handle, addr, cmd_rx, out_tx) = spawn_server(config, 16).await;
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.stream_observations = false;
    support::write_json_line(&mut write_half, &hello).await;

    let welcome = read_json_line(&mut lines).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["seq"], 1);

    // First command ok.
    let cmd1 = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_half, cmd1).await;

    let ack = read_json_line(&mut lines).await;
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["seq"], 2);
    // Engine harness follows with an observation.
    let obs1 = read_json_line(&mut lines).await;
    assert_eq!(obs1["type"], "observation");

    // Duplicate seq must be rejected.
    let cmd_dup = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_half, cmd_dup).await;

    let err = read_json_line(&mut lines).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "invalid_command");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_parse_error_returns_invalid_command() {
    let config = support::server_config();

    let (server_handle, addr, _cmd_rx, _out_tx) = spawn_server(config, 8).await;

    let (mut lines, mut write_half) = support::connect(addr).await;

    write_half.write_all(b"{not json\n").await.unwrap();
    write_half.flush().await.unwrap();

    let v = read_json_line(&mut lines).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["code"], "invalid_command");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_observer_enforcement_not_controller() {
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

    // Client 1 (becomes controller)
    let s1 = TcpStream::connect(addr).await.unwrap();
    let (r1, mut w1) = s1.into_split();
    let mut l1 = BufReader::new(r1).lines();
    let hello1 = create_hello(1, "c1", "3.0.0");
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
    let hello2 = create_hello(1, "c2", "3.0.0");
    w2.write_all(serde_json::to_string(&hello2).unwrap().as_bytes())
        .await
        .unwrap();
    w2.write_all(b"\n").await.unwrap();
    w2.flush().await.unwrap();
    let _ = read_json_line(&mut l2).await;

    // Observer tries to send a command.
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut w2, cmd).await;

    let v = read_json_line(&mut l2).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["seq"], 2);
    assert_eq!(v["code"], "not_controller");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_ready_probe_welcome_then_playable_observation() {
    let config = support::server_config();

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello requesting place + streaming observations
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.stream_observations = true;
    hello.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    support::write_json_line(&mut write_half, &hello).await;

    let welcome = read_json_line(&mut lines).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["seq"], 1);
    assert!(welcome.get("capabilities").is_some());
    assert_eq!(
        welcome["capabilities"]["control_policy"]["auto_promote_on_disconnect"],
        true
    );
    assert_eq!(
        welcome["capabilities"]["control_policy"]["promotion_order"],
        "lowest_client_id"
    );

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
async fn acceptance_place_invalid_rotation_returns_invalid_place() {
    let config = support::server_config_with_capacity(16);

    let (cmd_tx, _cmd_rx) = mpsc::channel::<InboundCommand>(16);
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

    // hello (place mode)
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    hello.requested.stream_observations = false;
    support::write_json_line(&mut write_half, &hello).await;
    let _welcome = read_json_line(&mut lines).await;

    // invalid rotation string should be rejected by the server mapping layer.
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"place","place":{"x":3,"rotation":"northeast","useHold":false}}"#;
    support::write_raw_line(&mut write_half, cmd).await;

    let err = read_json_line(&mut lines).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "invalid_place");

    server_handle.abort();
}

#[tokio::test]
async fn acceptance_place_rejected_when_paused() {
    let config = support::server_config_with_capacity(16);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello (place mode)
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    support::write_json_line(&mut write_half, &hello).await;

    let _welcome = read_json_line(&mut lines).await;
    let obs0 = read_json_line(&mut lines).await;
    assert_eq!(obs0["type"], "observation");

    // pause
    let cmd_pause = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["pause"]}"#;
    support::write_raw_line(&mut write_half, cmd_pause).await;
    let ack_pause = read_json_line(&mut lines).await;
    assert_eq!(ack_pause["type"], "ack");
    assert_eq!(ack_pause["seq"], 2);
    let obs_paused = read_json_line(&mut lines).await;
    assert_eq!(obs_paused["type"], "observation");
    assert_eq!(obs_paused["paused"], true);

    // place while paused must be rejected (invalid_place).
    let cmd_place = r#"{"type":"command","seq":3,"ts":1,"mode":"place","place":{"x":3,"rotation":"north","useHold":false}}"#;
    support::write_raw_line(&mut write_half, cmd_place).await;

    let err = read_json_line(&mut lines).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 3);
    assert_eq!(err["code"], "invalid_place");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_place_from_observer_returns_not_controller() {
    let config = support::server_config_with_capacity(16);

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

    // Client A (controller).
    let (mut lines_a, mut write_a) = support::connect(addr).await;
    let mut hello_a = create_hello(1, "acceptance-a", "3.0.0");
    hello_a.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    hello_a.requested.stream_observations = false;
    support::write_json_line(&mut write_a, &hello_a).await;
    let _welcome_a = read_json_line(&mut lines_a).await;

    // Client B (observer).
    let (mut lines_b, mut write_b) = support::connect(addr).await;
    let mut hello_b = create_hello(1, "acceptance-b", "3.0.0");
    hello_b.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    hello_b.requested.stream_observations = false;
    support::write_json_line(&mut write_b, &hello_b).await;
    let _welcome_b = read_json_line(&mut lines_b).await;

    // Observer attempts place -> not_controller.
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"place","place":{"x":3,"rotation":"north","useHold":false}}"#;
    support::write_raw_line(&mut write_b, cmd).await;

    let err = read_json_line(&mut lines_b).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "not_controller");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_place_rejected_when_game_over() {
    let config = support::server_config_with_capacity(16);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello (place mode, stream observations so we get game_over snapshot)
    let mut hello = create_hello(1, "acceptance", "3.0.0");
    hello.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    hello.requested.stream_observations = true;
    support::write_json_line(&mut write_half, &hello).await;

    let _welcome = read_json_line(&mut lines).await;
    let obs0 = read_json_line(&mut lines).await;
    assert_eq!(obs0["type"], "observation");
    assert_eq!(obs0["game_over"], true);
    assert_eq!(obs0["playable"], false);

    // place while game_over must be rejected as invalid_place (not playable).
    let cmd = r#"{"type":"command","seq":2,"ts":1,"mode":"place","place":{"x":3,"rotation":"north","useHold":false}}"#;
    support::write_raw_line(&mut write_half, cmd).await;

    let err = read_json_line(&mut lines).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "invalid_place");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_place_rejected_when_no_controller_until_claim() {
    let config = support::server_config_with_capacity(16);

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

    // Client A (controller).
    let (mut lines_a, mut write_a) = support::connect(addr).await;
    let mut hello_a = create_hello(1, "acceptance-a", "3.0.0");
    hello_a.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    hello_a.requested.stream_observations = false;
    support::write_json_line(&mut write_a, &hello_a).await;
    let _welcome_a = read_json_line(&mut lines_a).await;

    // Client B (observer).
    let (mut lines_b, mut write_b) = support::connect(addr).await;
    let mut hello_b = create_hello(1, "acceptance-b", "3.0.0");
    hello_b.requested.command_mode = tetris_adapter_protocol::protocol::CommandMode::Place;
    hello_b.requested.stream_observations = false;
    support::write_json_line(&mut write_b, &hello_b).await;
    let _welcome_b = read_json_line(&mut lines_b).await;

    // Controller releases (no controller assigned).
    let release_a = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    support::write_raw_line(&mut write_a, release_a).await;
    let ack_release = read_json_line(&mut lines_a).await;
    assert_eq!(ack_release["type"], "ack");
    assert_eq!(ack_release["seq"], 2);

    // Place while no controller must be rejected.
    let place_b = r#"{"type":"command","seq":2,"ts":1,"mode":"place","place":{"x":3,"rotation":"north","useHold":false}}"#;
    support::write_raw_line(&mut write_b, place_b).await;
    let err = read_json_line(&mut lines_b).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "not_controller");

    // Claim and try again.
    let claim_b = r#"{"type":"control","seq":3,"ts":1,"action":"claim"}"#;
    support::write_raw_line(&mut write_b, claim_b).await;
    let ack_claim = read_json_line(&mut lines_b).await;
    assert_eq!(ack_claim["type"], "ack");
    assert_eq!(ack_claim["seq"], 3);

    let place_b2 = r#"{"type":"command","seq":4,"ts":1,"mode":"place","place":{"x":3,"rotation":"north","useHold":false}}"#;
    support::write_raw_line(&mut write_b, place_b2).await;

    let resp = read_json_line(&mut lines_b).await;
    assert_eq!(resp["type"], "ack");
    assert_eq!(resp["seq"], 4);

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_restart_and_pause_semantics() {
    let config = support::server_config();

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello
    let hello = create_hello(1, "acceptance", "3.0.0");
    support::write_json_line(&mut write_half, &hello).await;

    let _welcome = read_json_line(&mut lines).await;
    let _obs0 = read_json_line(&mut lines).await;

    // pause
    let cmd_pause = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["pause"]}"#;
    support::write_raw_line(&mut write_half, cmd_pause).await;

    let ack = read_json_line(&mut lines).await;
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["seq"], 2);

    let obs = read_json_line(&mut lines).await;
    assert_eq!(obs["type"], "observation");
    assert_eq!(obs["paused"], true);
    assert_eq!(obs["playable"], false);

    // restart
    let cmd_restart = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["restart"]}"#;
    support::write_raw_line(&mut write_half, cmd_restart).await;

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
    let config = support::server_config_with_capacity(16);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello
    let hello = create_hello(1, "acceptance", "3.0.0");
    support::write_json_line(&mut write_half, &hello).await;

    let _welcome = read_json_line(&mut lines).await;
    let obs0 = read_json_line(&mut lines).await;
    assert_eq!(obs0["type"], "observation");
    assert_eq!(obs0["paused"], false);

    // pause
    let cmd_pause = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["pause"]}"#;
    support::write_raw_line(&mut write_half, cmd_pause).await;

    let _ack_pause = read_json_line(&mut lines).await;
    let obs_paused = read_json_line(&mut lines).await;
    assert_eq!(obs_paused["type"], "observation");
    assert_eq!(obs_paused["paused"], true);

    let paused_hash = obs_paused["state_hash"].as_str().unwrap().to_string();
    let paused_piece_id = obs_paused["piece_id"].as_u64().unwrap();
    let paused_board_id = obs_paused["board_id"].as_u64().unwrap();
    let paused_active_x = obs_paused["active"]["x"].as_i64().unwrap();
    let paused_active_y = obs_paused["active"]["y"].as_i64().unwrap();

    // moveLeft while paused should be ignored.
    let cmd_move = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_half, cmd_move).await;

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
    let config = support::server_config_with_capacity(16);

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

    let (mut lines, mut write_half) = support::connect(addr).await;

    // hello
    let hello = create_hello(1, "acceptance", "3.0.0");
    support::write_json_line(&mut write_half, &hello).await;

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

    // moveLeft while game_over should be ignored.
    let cmd_move = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_half, cmd_move).await;

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
    support::write_raw_line(&mut write_half, cmd_restart).await;

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
    let config = support::server_config_with_capacity(16);

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
    let (mut lines_a, mut write_a) = support::connect(addr).await;

    let hello_a = create_hello(1, "acceptance-a", "3.0.0");
    support::write_json_line(&mut write_a, &hello_a).await;

    let welcome_a = read_json_line(&mut lines_a).await;
    assert_eq!(welcome_a["type"], "welcome");
    let obs_a0 = read_json_line(&mut lines_a).await;
    assert_eq!(obs_a0["type"], "observation");

    // Client B (observer).
    let (mut lines_b, mut write_b) = support::connect(addr).await;

    let hello_b = create_hello(1, "acceptance-b", "3.0.0");
    support::write_json_line(&mut write_b, &hello_b).await;

    let welcome_b = read_json_line(&mut lines_b).await;
    assert_eq!(welcome_b["type"], "welcome");
    let obs_b0 = read_json_line(&mut lines_b).await;
    assert_eq!(obs_b0["type"], "observation");

    // Observer cannot send commands.
    let cmd_b = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_b, cmd_b).await;

    let err_b = read_json_line(&mut lines_b).await;
    assert_eq!(err_b["type"], "error");
    assert_eq!(err_b["seq"], 2);
    assert_eq!(err_b["code"], "not_controller");

    // Observer cannot claim while controller is active.
    let claim_b = r#"{"type":"control","seq":3,"ts":1,"action":"claim"}"#;
    support::write_raw_line(&mut write_b, claim_b).await;

    let err_claim = read_json_line(&mut lines_b).await;
    assert_eq!(err_claim["type"], "error");
    assert_eq!(err_claim["seq"], 3);
    assert_eq!(err_claim["code"], "controller_active");

    // Controller releases.
    let release_a = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    support::write_raw_line(&mut write_a, release_a).await;

    let ack_release = read_json_line(&mut lines_a).await;
    assert_eq!(ack_release["type"], "ack");
    assert_eq!(ack_release["seq"], 2);

    // Observer can claim now.
    let claim_b2 = r#"{"type":"control","seq":4,"ts":1,"action":"claim"}"#;
    support::write_raw_line(&mut write_b, claim_b2).await;

    let ack_claim = read_json_line(&mut lines_b).await;
    assert_eq!(ack_claim["type"], "ack");
    assert_eq!(ack_claim["seq"], 4);

    // New controller can send a command (ack comes from the engine task).
    let cmd_b2 = r#"{"type":"command","seq":5,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_b, cmd_b2).await;

    let ack_cmd = read_json_line(&mut lines_b).await;
    assert_eq!(ack_cmd["type"], "ack");
    assert_eq!(ack_cmd["seq"], 5);
    let obs_b1 = read_json_line(&mut lines_b).await;
    assert_eq!(obs_b1["type"], "observation");

    // Old controller cannot send commands anymore.
    let cmd_a = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_a, cmd_a).await;

    let err_a = read_json_line(&mut lines_a).await;
    assert_eq!(err_a["type"], "error");
    assert_eq!(err_a["seq"], 3);
    assert_eq!(err_a["code"], "not_controller");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_control_release_requires_controller() {
    let config = support::server_config_with_capacity(16);

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
    let (mut lines_a, mut write_a) = support::connect(addr).await;

    let hello_a = create_hello(1, "acceptance-a", "3.0.0");
    support::write_json_line(&mut write_a, &hello_a).await;
    let _welcome_a = read_json_line(&mut lines_a).await;
    let _obs_a0 = read_json_line(&mut lines_a).await;

    // Client B (observer).
    let (mut lines_b, mut write_b) = support::connect(addr).await;

    let hello_b = create_hello(1, "acceptance-b", "3.0.0");
    support::write_json_line(&mut write_b, &hello_b).await;
    let _welcome_b = read_json_line(&mut lines_b).await;
    let _obs_b0 = read_json_line(&mut lines_b).await;

    // Non-controller release must be rejected.
    let release_b = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    support::write_raw_line(&mut write_b, release_b).await;

    let err = read_json_line(&mut lines_b).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["seq"], 2);
    assert_eq!(err["code"], "not_controller");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_no_controller_after_release_rejects_commands_until_claim() {
    let config = support::server_config_with_capacity(16);

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
    let (mut lines_a, mut write_a) = support::connect(addr).await;

    let hello_a = create_hello(1, "acceptance-a", "3.0.0");
    support::write_json_line(&mut write_a, &hello_a).await;
    let _welcome_a = read_json_line(&mut lines_a).await;
    let _obs_a0 = read_json_line(&mut lines_a).await;

    // Client B (observer).
    let (mut lines_b, mut write_b) = support::connect(addr).await;

    let hello_b = create_hello(1, "acceptance-b", "3.0.0");
    support::write_json_line(&mut write_b, &hello_b).await;
    let _welcome_b = read_json_line(&mut lines_b).await;
    let _obs_b0 = read_json_line(&mut lines_b).await;

    // Controller releases.
    let release_a = r#"{"type":"control","seq":2,"ts":1,"action":"release"}"#;
    support::write_raw_line(&mut write_a, release_a).await;
    let ack_release = read_json_line(&mut lines_a).await;
    assert_eq!(ack_release["type"], "ack");
    assert_eq!(ack_release["seq"], 2);

    // With no controller assigned, commands from either client must be rejected.
    let cmd_a = r#"{"type":"command","seq":3,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_a, cmd_a).await;
    let err_a = read_json_line(&mut lines_a).await;
    assert_eq!(err_a["type"], "error");
    assert_eq!(err_a["seq"], 3);
    assert_eq!(err_a["code"], "not_controller");

    let cmd_b = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_b, cmd_b).await;
    let err_b = read_json_line(&mut lines_b).await;
    assert_eq!(err_b["type"], "error");
    assert_eq!(err_b["seq"], 2);
    assert_eq!(err_b["code"], "not_controller");

    // Observer claims controller, then commands should succeed.
    let claim_b = r#"{"type":"control","seq":3,"ts":1,"action":"claim"}"#;
    support::write_raw_line(&mut write_b, claim_b).await;
    let ack_claim = read_json_line(&mut lines_b).await;
    assert_eq!(ack_claim["type"], "ack");
    assert_eq!(ack_claim["seq"], 3);

    let cmd_b2 = r#"{"type":"command","seq":4,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_b, cmd_b2).await;
    let ack_cmd = read_json_line(&mut lines_b).await;
    assert_eq!(ack_cmd["type"], "ack");
    assert_eq!(ack_cmd["seq"], 4);
    let obs_b1 = read_json_line(&mut lines_b).await;
    assert_eq!(obs_b1["type"], "observation");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_control_claim_is_idempotent_for_controller() {
    let config = support::server_config_with_capacity(16);

    let (server_handle, addr, cmd_rx, out_tx) = spawn_server(config, 16).await;
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let (mut lines_a, mut write_a) = support::connect(addr).await;

    let hello_a = create_hello(1, "acceptance-a", "3.0.0");
    support::write_json_line(&mut write_a, &hello_a).await;

    let welcome_a = read_json_line(&mut lines_a).await;
    assert_eq!(welcome_a["type"], "welcome");
    assert_eq!(welcome_a["role"], "controller");
    assert_ne!(welcome_a["client_id"], serde_json::Value::Null);
    assert_eq!(welcome_a["controller_id"], welcome_a["client_id"]);

    let _obs_a0 = read_json_line(&mut lines_a).await;

    // Claim again as controller should be idempotent (ack, not controller_active).
    let claim_a = r#"{"type":"control","seq":2,"ts":1,"action":"claim"}"#;
    support::write_raw_line(&mut write_a, claim_a).await;

    let ack_claim = read_json_line(&mut lines_a).await;
    assert_eq!(ack_claim["type"], "ack");
    assert_eq!(ack_claim["seq"], 2);

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_requested_role_observer_never_auto_becomes_controller() {
    let config = support::server_config_with_capacity(16);

    let (server_handle, addr, cmd_rx, out_tx) = spawn_server(config, 16).await;
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let (mut lines_a, mut write_a) = support::connect(addr).await;

    // Request observer role in hello; this MUST NOT auto-assign controller as a side-effect of hello.
    let hello_a = serde_json::json!({
        "type": "hello",
        "seq": 1,
        "ts": 1,
        "client": {"name": "acceptance-a", "version": "0.1.0"},
        "protocol_version": "3.0.0",
        "formats": ["json"],
        "requested": {
            "stream_observations": true,
            "command_mode": "action",
            "role": "observer"
        }
    });
    support::write_json_line(&mut write_a, &hello_a).await;

    let welcome_a = read_json_line(&mut lines_a).await;
    assert_eq!(welcome_a["type"], "welcome");
    assert_eq!(welcome_a["role"], "observer");
    assert_ne!(welcome_a["client_id"], serde_json::Value::Null);
    assert_eq!(welcome_a["controller_id"], serde_json::Value::Null);

    let _obs_a0 = read_json_line(&mut lines_a).await;

    // With no controller assigned, commands must be rejected.
    let cmd_a = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_a, cmd_a).await;

    let err_a = read_json_line(&mut lines_a).await;
    assert_eq!(err_a["type"], "error");
    assert_eq!(err_a["seq"], 2);
    assert_eq!(err_a["code"], "not_controller");

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_observer_connected_does_not_block_new_controller_hello() {
    let config = support::server_config_with_capacity(16);

    let (server_handle, addr, cmd_rx, out_tx) = spawn_server(config, 16).await;
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    // Observer connects first.
    let (mut lines_obs, mut write_obs) = support::connect(addr).await;

    let hello_obs = serde_json::json!({
        "type": "hello",
        "seq": 1,
        "ts": 1,
        "client": {"name": "observe", "version": "0.1.0"},
        "protocol_version": "3.0.0",
        "formats": ["json"],
        "requested": {
            "stream_observations": true,
            "command_mode": "action",
            "role": "observer"
        }
    });
    support::write_json_line(&mut write_obs, &hello_obs).await;

    let welcome_obs = read_json_line(&mut lines_obs).await;
    assert_eq!(welcome_obs["type"], "welcome");
    assert_eq!(welcome_obs["role"], "observer");
    assert_eq!(welcome_obs["controller_id"], serde_json::Value::Null);
    let _obs0 = read_json_line(&mut lines_obs).await;

    // Controller client connects after observer and should become controller.
    let (mut lines_ai, mut write_ai) = support::connect(addr).await;

    let hello_ai = create_hello(1, "ai-client", "3.0.0");
    support::write_json_line(&mut write_ai, &hello_ai).await;

    let welcome_ai = read_json_line(&mut lines_ai).await;
    assert_eq!(welcome_ai["type"], "welcome");
    assert_eq!(welcome_ai["role"], "controller");
    assert_eq!(welcome_ai["controller_id"], welcome_ai["client_id"]);

    // Controller command should be accepted (ack from harness engine).
    let cmd_ai = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_ai, cmd_ai).await;

    let mut saw_ack = false;
    for _ in 0..10 {
        let v = read_json_line(&mut lines_ai).await;
        if v["type"] == "ack" && v["seq"] == 2 {
            saw_ack = true;
            break;
        }
    }
    assert!(
        saw_ack,
        "controller should be able to command while observer stays connected"
    );

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_controller_disconnect_promotes_next_client() {
    let config = support::server_config_with_capacity(16);

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
    let (mut lines_a, mut write_a) = support::connect(addr).await;

    let hello_a = create_hello(1, "acceptance-a", "3.0.0");
    support::write_json_line(&mut write_a, &hello_a).await;

    let welcome_a = read_json_line(&mut lines_a).await;
    assert_eq!(welcome_a["type"], "welcome");
    let _obs_a0 = read_json_line(&mut lines_a).await;

    // Client B (observer initially).
    let (mut lines_b, mut write_b) = support::connect(addr).await;

    let hello_b = create_hello(1, "acceptance-b", "3.0.0");
    support::write_json_line(&mut write_b, &hello_b).await;

    let welcome_b = read_json_line(&mut lines_b).await;
    assert_eq!(welcome_b["type"], "welcome");
    let _obs_b0 = read_json_line(&mut lines_b).await;

    // Disconnect controller A.
    drop(write_a);
    drop(lines_a);

    // B should be promoted to controller and commands should succeed.
    let cmd_b = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_b, cmd_b).await;

    // We may see an observation before the ack depending on scheduling; scan a few frames.
    let mut saw_ack = false;
    for _ in 0..10 {
        let v = read_json_line(&mut lines_b).await;
        if v["type"] == "ack" && v["seq"] == 2 {
            saw_ack = true;
            break;
        }
    }
    assert!(
        saw_ack,
        "expected ack after controller disconnect promotion"
    );

    server_handle.abort();
    engine_handle.abort();
}

#[tokio::test]
async fn acceptance_controller_disconnect_does_not_auto_promote_observer_role() {
    let config = support::server_config_with_capacity(16);

    let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(16);
    let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
    let (ready_tx, ready_rx) = oneshot::channel();
    let (status_tx, mut status_rx) =
        watch::channel(tetris_adapter::adapter::runtime::AdapterStatus {
            client_count: 0,
            controller_id: None,
            streaming_count: 0,
        });

    let server_handle = tokio::spawn(async move {
        let _ = run_server(config, cmd_tx, out_rx, Some(ready_tx), Some(status_tx)).await;
    });
    let engine_handle = tokio::spawn(engine_task(cmd_rx, out_tx));

    let addr = tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .unwrap()
        .unwrap();

    // Client A (controller by default).
    let (mut lines_a, mut write_a) = support::connect(addr).await;

    let hello_a = create_hello(1, "acceptance-a", "3.0.0");
    support::write_json_line(&mut write_a, &hello_a).await;
    let _welcome_a = read_json_line(&mut lines_a).await;
    let _obs_a0 = read_json_line(&mut lines_a).await;

    // Client B requests observer role.
    let (mut lines_b, mut write_b) = support::connect(addr).await;

    let hello_b = serde_json::json!({
        "type": "hello",
        "seq": 1,
        "ts": 1,
        "client": {"name": "acceptance-b", "version": "0.1.0"},
        "protocol_version": "3.0.0",
        "formats": ["json"],
        "requested": {
            "stream_observations": true,
            "command_mode": "action",
            "role": "observer"
        }
    });
    support::write_json_line(&mut write_b, &hello_b).await;
    let welcome_b = read_json_line(&mut lines_b).await;
    assert_eq!(welcome_b["role"], "observer");
    let _obs_b0 = read_json_line(&mut lines_b).await;

    // Disconnect controller A.
    drop(write_a);
    drop(lines_a);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if status_rx.borrow().controller_id.is_none() {
                break;
            }
            status_rx.changed().await.unwrap();
        }
    })
    .await
    .expect("controller cleanup timed out");

    // Observer B must remain non-controller after auto-promotion attempt.
    let cmd_b = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft"]}"#;
    support::write_raw_line(&mut write_b, cmd_b).await;

    let mut saw_not_controller = false;
    for _ in 0..10 {
        let v = read_json_line(&mut lines_b).await;
        if v["type"] == "error" && v["seq"] == 2 && v["code"] == "not_controller" {
            saw_not_controller = true;
            break;
        }
    }
    assert!(
        saw_not_controller,
        "observer must not be auto-promoted on disconnect"
    );

    // Observer can still explicitly claim and then command.
    let claim_b = r#"{"type":"control","seq":3,"ts":1,"action":"claim"}"#;
    support::write_raw_line(&mut write_b, claim_b).await;
    let ack_claim = read_json_line(&mut lines_b).await;
    assert_eq!(ack_claim["type"], "ack");
    assert_eq!(ack_claim["seq"], 3);

    let cmd_b2 = r#"{"type":"command","seq":4,"ts":1,"mode":"action","actions":["moveRight"]}"#;
    support::write_raw_line(&mut write_b, cmd_b2).await;

    let mut saw_ack = false;
    for _ in 0..10 {
        let v = read_json_line(&mut lines_b).await;
        if v["type"] == "ack" && v["seq"] == 4 {
            saw_ack = true;
            break;
        }
    }
    assert!(saw_ack, "expected ack after explicit claim");

    server_handle.abort();
    engine_handle.abort();
}
