use super::*;
use tetris_core::core::GameState;

#[test]
fn protocol_compatibility_requires_semver_with_major_three() {
    assert!(is_compatible_protocol_version("3.0.0"));
    assert!(is_compatible_protocol_version("3.1.0-beta.1"));
    assert!(!is_compatible_protocol_version("2.1.1"));
    assert!(!is_compatible_protocol_version("2.invalid"));
    assert!(!is_compatible_protocol_version("2.1"));
    assert!(!is_compatible_protocol_version("2.1.0-"));
    assert!(!is_compatible_protocol_version("2.1.0-01"));
}

#[test]
fn test_map_command_action_mode() {
    let json = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft","rotateCw","hardDrop"]}"#;
    let ParsedMessage::Command(cmd) = parse_message(json).unwrap() else {
        panic!("expected command");
    };
    let mapped = map_command(&cmd).unwrap();
    match mapped {
        ClientCommand::Actions {
            actions,
            restart_seed,
        } => {
            assert_eq!(
                actions.as_slice(),
                [
                    GameAction::MoveLeft,
                    GameAction::RotateCw,
                    GameAction::HardDrop
                ]
            );
            assert_eq!(restart_seed, None);
        }
        _ => panic!("expected action mapping"),
    }
}

#[test]
fn test_build_observation_copies_timers_fields() {
    let mut snap = tetris_core::core::snapshot::GameSnapshot::default();
    snap.timers.drop_ms = 12;
    snap.timers.lock_ms = 34;
    snap.timers.line_clear_ms = 56;

    let obs = build_observation(1, 0, &snap, &[]);
    assert_eq!(obs.timers.drop_ms, 12);
    assert_eq!(obs.timers.lock_ms, 34);
    assert_eq!(obs.timers.line_clear_ms, 56);
}

#[test]
fn test_server_config_from_env() {
    // This test just ensures it doesn't panic
    let _config = ServerConfig::from_env();
}

#[tokio::test]
async fn invalid_host_returns_error_instead_of_panicking() {
    let config = ServerConfig {
        host: "not a valid host name !!!".to_string(),
        port: 7777,
        ..ServerConfig::default()
    };
    let (command_tx, _command_rx) = mpsc::channel(1);
    let (_out_tx, out_rx) = mpsc::unbounded_channel();

    let result = run_server(config, command_tx, out_rx, None, None).await;
    assert!(result.is_err());
}

#[test]
fn test_clear_stale_controller_id_clears() {
    let mut controller = Some(42usize);
    clear_stale_controller_id(&mut controller, |id| id == 7);
    assert_eq!(controller, None);
}

#[test]
fn test_clear_stale_controller_id_keeps() {
    let mut controller = Some(42usize);
    clear_stale_controller_id(&mut controller, |id| id == 42);
    assert_eq!(controller, Some(42));
}

#[test]
fn test_encode_json_into_buf_ack() {
    let ack = create_ack(10, 10);
    let mut buf = Vec::new();
    assert!(encode_json_into_buf(&mut buf, &ack));
    let text = std::str::from_utf8(&buf).unwrap();
    assert!(text.contains("\"type\":\"ack\""));
    assert!(text.contains("\"seq\":10"));
}

#[test]
fn broker_disconnect_promotes_lowest_eligible_client() {
    let addr = "127.0.0.1:9999".parse().unwrap();
    let (tx1, rx1, obs1, shutdown1) = client_outbound_channel(1);
    let (tx2, rx2, obs2, shutdown2) = client_outbound_channel(1);
    let _receiver_guards = (rx1, obs1, shutdown1, rx2, obs2, shutdown2);
    let clients = vec![
        ClientHandle {
            id: 1,
            addr,
            requested_role: RequestedRole::Auto,
            command_mode: CommandMode::Action,
            stream_observations: false,
            handshaken: true,
            last_seq: Some(1),
            outbound: tx1,
        },
        ClientHandle {
            id: 2,
            addr,
            requested_role: RequestedRole::Auto,
            command_mode: CommandMode::Action,
            stream_observations: false,
            handshaken: true,
            last_seq: Some(1),
            outbound: tx2,
        },
    ];

    let mut broker = BrokerState {
        clients,
        controller_id: Some(1),
    };
    broker.remove_and_promote(1);

    assert_eq!(broker.controller_id, Some(2));
    assert_eq!(broker.clients.len(), 1);
}

#[test]
fn broker_authorization_has_one_controller_source_of_truth() {
    let mut broker = BrokerState {
        controller_id: Some(7),
        ..BrokerState::default()
    };

    assert!(broker.is_controller(7));
    assert!(!broker.is_controller(6));
    broker.controller_id = None;
    assert!(!broker.is_controller(7));
}

#[test]
fn test_state_hash_changes_when_meta_changes() {
    let mut gs = GameState::new(1);
    gs.start();

    let mut s1 = gs.snapshot();
    s1.episode_id = 0;
    s1.piece_id = 1;
    s1.step_in_piece = 0;
    let obs1 = build_observation(1, 0, &s1, &[]);

    let mut s2 = gs.snapshot();
    s2.episode_id = 1;
    s2.piece_id = 2;
    s2.step_in_piece = 3;
    let obs2 = build_observation(2, 0, &s2, &[]);
    assert_ne!(obs1.state_hash, obs2.state_hash);
}

#[test]
fn test_state_hash_changes_when_hold_changes() {
    let mut gs = GameState::new(1);
    gs.start();

    let s1 = gs.snapshot();
    let obs1 = build_observation(1, 0, &s1, &[]);
    assert!(gs.apply_action(GameAction::Hold));

    let s2 = gs.snapshot();
    let obs2 = build_observation(2, 0, &s2, &[]);
    assert_ne!(obs1.state_hash, obs2.state_hash);
}

#[test]
fn test_state_hash_changes_when_board_id_changes() {
    let mut gs = GameState::new(1);
    gs.start();

    let s1 = gs.snapshot();
    let obs1 = build_observation(1, 0, &s1, &[]);

    let mut s2 = s1;
    s2.board_id = s2.board_id.wrapping_add(1);
    let obs2 = build_observation(2, 0, &s2, &[]);

    assert_ne!(obs1.state_hash, obs2.state_hash);
}
