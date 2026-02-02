#[test]
fn adapter_protocol_schema_is_valid_json() {
    let s = std::fs::read_to_string("docs/adapter-protocol.schema.json")
        .expect("read docs/adapter-protocol.schema.json");
    let v: serde_json::Value = serde_json::from_str(&s).expect("schema must be valid json");
    assert_eq!(v["title"], "Tetris AI Adapter Protocol");
    assert!(v.get("definitions").is_some());
}

#[test]
fn adapter_protocol_smoke_messages_parse() {
    // hello
    let hello = r#"{"type":"hello","seq":1,"ts":1,"client":{"name":"t","version":"0"},"protocol_version":"2.0.0","formats":["json"],"requested":{"stream_observations":true,"command_mode":"place"}}"#;
    let _ = tui_tetris::adapter::protocol::parse_message(hello).unwrap();

    // welcome
    let welcome = tui_tetris::adapter::protocol::create_welcome(1, "2.0.0");
    let _ = serde_json::to_string(&welcome).unwrap();

    // observation (built from defaults)
    let mut gs = tui_tetris::core::GameState::new(1);
    gs.start();
    let mut snap = tui_tetris::core::GameSnapshot::default();
    gs.snapshot_into(&mut snap);
    let obs = tui_tetris::adapter::server::build_observation(1, &snap, None);
    let json = serde_json::to_string(&obs).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "observation");
    assert!(v.get("board_id").is_some());
    assert!(v.get("ghost_y").is_some());
}
