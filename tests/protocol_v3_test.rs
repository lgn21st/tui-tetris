use tetris_adapter::adapter::observation::build_observation;
use tetris_adapter_protocol::protocol::{
    PROTOCOL_VERSION, StateHash, TransitionEvent, create_applied_ack,
};
use tetris_core::core::GameState;
use tetris_core::types::{CoreLastEvent, TSpinKind};

fn event(lines: u32) -> TransitionEvent {
    CoreLastEvent {
        locked: true,
        lines_cleared: lines,
        line_clear_score: lines * 100,
        tspin: Some(TSpinKind::Full),
        combo: 1,
        back_to_back: true,
    }
    .into()
}

#[test]
fn v3_observation_exposes_logical_step_and_all_events() {
    assert_eq!(PROTOCOL_VERSION, "3.0.0");
    let mut game = GameState::new(3);
    game.start();
    let observation = build_observation(8, 21, &game.snapshot(), &[event(1), event(2)]);
    let json = serde_json::to_value(observation).unwrap();

    assert_eq!(json["logical_step"], 21);
    assert_eq!(json["events"].as_array().unwrap().len(), 2);
    assert_eq!(json["events"][1]["lines_cleared"], 2);
    assert!(json.get("last_event").is_none());
}

#[test]
fn applied_ack_correlates_command_with_authoritative_state() {
    let ack = create_applied_ack(11, 7, 42, StateHash(0xabc));
    let json = serde_json::to_value(ack).unwrap();

    assert_eq!(json["seq"], 11);
    assert_eq!(json["correlation_seq"], 7);
    assert_eq!(json["applied_step"], 42);
    assert_eq!(json["state_hash"], "0000000000000abc");
}
