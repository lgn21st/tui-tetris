#[test]
fn adapter_protocol_schema_is_valid_json() {
    let s = std::fs::read_to_string("docs/adapter-protocol.schema.json")
        .expect("read docs/adapter-protocol.schema.json");
    let v: serde_json::Value = serde_json::from_str(&s).expect("schema must be valid json");
    assert_eq!(v["title"], "Tetris AI Adapter Protocol");
    assert_eq!(v["$schema"], "http://json-schema.org/draft-07/schema#");
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

#[test]
fn adapter_protocol_schema_has_expected_definitions() {
    let s = std::fs::read_to_string("docs/adapter-protocol.schema.json")
        .expect("read docs/adapter-protocol.schema.json");
    let v: serde_json::Value = serde_json::from_str(&s).expect("schema must be valid json");

    let defs = v
        .get("definitions")
        .and_then(|d| d.as_object())
        .expect("schema must have definitions object");

    for k in [
        "hello",
        "welcome",
        "command",
        "control",
        "ack",
        "error",
        "observation",
        "board",
        "active_piece",
        "place",
        "timers",
        "last_event",
    ] {
        assert!(defs.contains_key(k), "missing schema definition: {k}");
    }
}

#[test]
fn adapter_protocol_schema_encodes_board_shape_and_cell_range() {
    let s = std::fs::read_to_string("docs/adapter-protocol.schema.json")
        .expect("read docs/adapter-protocol.schema.json");
    let v: serde_json::Value = serde_json::from_str(&s).expect("schema must be valid json");

    let board = &v["definitions"]["board"]["properties"];
    assert_eq!(board["cells"]["type"], "array");
    assert_eq!(board["cells"]["minItems"], 20);
    assert_eq!(board["cells"]["maxItems"], 20);

    let row = &board["cells"]["items"];
    assert_eq!(row["type"], "array");
    assert_eq!(row["minItems"], 10);
    assert_eq!(row["maxItems"], 10);

    let cell = &row["items"];
    assert_eq!(cell["type"], "integer");
    assert_eq!(cell["minimum"], 0);
    assert_eq!(cell["maximum"], 7);
}

#[test]
fn adapter_protocol_schema_requires_next_queue_len_5() {
    let s = std::fs::read_to_string("docs/adapter-protocol.schema.json")
        .expect("read docs/adapter-protocol.schema.json");
    let v: serde_json::Value = serde_json::from_str(&s).expect("schema must be valid json");

    let next_queue = &v["definitions"]["observation"]["properties"]["next_queue"];
    assert_eq!(next_queue["type"], "array");
    assert_eq!(next_queue["minItems"], 5);
    assert_eq!(next_queue["maxItems"], 5);
}
