//! Protocol module - JSON message types for AI adapter
//!
//! Implements the line-delimited JSON protocol compatible with swiftui-tetris.
//! All messages have: type, seq (sequence number), ts (timestamp in ms)

use serde::{Deserialize, Serialize};

use crate::types::{CoreLastEvent, PieceKind, Rotation, TSpinKind};

use arrayvec::ArrayVec;

// ============== Client -> Game Messages ==============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HelloType {
    #[serde(rename = "hello")]
    Hello,
}

impl Default for HelloType {
    fn default() -> Self {
        Self::Hello
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandType {
    #[serde(rename = "command")]
    Command,
}

impl Default for CommandType {
    fn default() -> Self {
        Self::Command
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlType {
    #[serde(rename = "control")]
    Control,
}

impl Default for ControlType {
    fn default() -> Self {
        Self::Control
    }
}

/// Client hello message (first message to establish connection)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMessage {
    #[serde(rename = "type")]
    #[serde(default)]
    pub msg_type: HelloType,
    pub seq: u64,
    pub ts: u64,
    pub client: ClientInfo,
    pub protocol_version: String,
    pub formats: FormatsList,
    pub requested: RequestedCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatsList {
    pub json: bool,
}

impl<'de> Deserialize<'de> for FormatsList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = FormatsList;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "an array of format strings")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut json = false;
                while let Some(v) = seq.next_element::<&str>()? {
                    if v.eq_ignore_ascii_case("json") {
                        json = true;
                    }
                }
                Ok(FormatsList { json })
            }
        }

        deserializer.deserialize_seq(V)
    }
}

impl Serialize for FormatsList {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(if self.json { 1 } else { 0 }))?;
        if self.json {
            seq.serialize_element("json")?;
        }
        seq.end()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestedCapabilities {
    #[serde(rename = "stream_observations")]
    pub stream_observations: bool,
    #[serde(rename = "command_mode")]
    pub command_mode: CommandMode,
    /// Optional role request for deterministic controller/observer negotiation.
    /// Per spec, this MUST NOT change role unless explicitly supported by the adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<RequestedRole>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestedRole {
    Auto,
    Controller,
    Observer,
}

impl<'de> Deserialize<'de> for RequestedRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        if s.eq_ignore_ascii_case("auto") {
            Ok(Self::Auto)
        } else if s.eq_ignore_ascii_case("controller") {
            Ok(Self::Controller)
        } else if s.eq_ignore_ascii_case("observer") {
            Ok(Self::Observer)
        } else {
            Err(serde::de::Error::custom("invalid requested role"))
        }
    }
}

impl Serialize for RequestedRole {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RequestedRole::Auto => serializer.serialize_str("auto"),
            RequestedRole::Controller => serializer.serialize_str("controller"),
            RequestedRole::Observer => serializer.serialize_str("observer"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssignedRole {
    #[serde(rename = "controller")]
    Controller,
    #[serde(rename = "observer")]
    Observer,
}

/// Command message (controller only)
#[derive(Debug, Clone, Deserialize)]
pub struct CommandMessage {
    #[serde(rename = "type")]
    #[serde(default)]
    pub msg_type: CommandType,
    pub seq: u64,
    pub ts: u64,
    pub mode: CommandMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<ActionList>, // For action mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceCommand>, // For place mode
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandMode {
    Action,
    Place,
}

impl<'de> Deserialize<'de> for CommandMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        if s.eq_ignore_ascii_case("action") {
            Ok(Self::Action)
        } else if s.eq_ignore_ascii_case("place") {
            Ok(Self::Place)
        } else {
            Err(serde::de::Error::custom("invalid command mode"))
        }
    }
}

impl Serialize for CommandMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            CommandMode::Action => serializer.serialize_str("action"),
            CommandMode::Place => serializer.serialize_str("place"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionName {
    MoveLeft,
    MoveRight,
    SoftDrop,
    HardDrop,
    RotateCw,
    RotateCcw,
    Hold,
    Pause,
    Restart,
}

impl<'de> Deserialize<'de> for ActionName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        if s.eq_ignore_ascii_case("moveLeft") {
            Ok(Self::MoveLeft)
        } else if s.eq_ignore_ascii_case("moveRight") {
            Ok(Self::MoveRight)
        } else if s.eq_ignore_ascii_case("softDrop") {
            Ok(Self::SoftDrop)
        } else if s.eq_ignore_ascii_case("hardDrop") {
            Ok(Self::HardDrop)
        } else if s.eq_ignore_ascii_case("rotateCw") {
            Ok(Self::RotateCw)
        } else if s.eq_ignore_ascii_case("rotateCcw") {
            Ok(Self::RotateCcw)
        } else if s.eq_ignore_ascii_case("hold") {
            Ok(Self::Hold)
        } else if s.eq_ignore_ascii_case("pause") {
            Ok(Self::Pause)
        } else if s.eq_ignore_ascii_case("restart") {
            Ok(Self::Restart)
        } else {
            Err(serde::de::Error::custom("unknown action"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionList(pub ArrayVec<ActionName, 32>);

impl<'de> Deserialize<'de> for ActionList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = ActionList;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "an array of action strings")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut out = ArrayVec::<ActionName, 32>::new();
                while let Some(a) = seq.next_element::<ActionName>()? {
                    out.try_push(a)
                        .map_err(|_| serde::de::Error::custom("too many actions"))?;
                }
                Ok(ActionList(out))
            }
        }

        deserializer.deserialize_seq(V)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaceCommand {
    pub x: i8,
    pub rotation: String,
    #[serde(rename = "useHold")]
    pub use_hold: bool,
}

/// Control message (claim/release controller status)
#[derive(Debug, Clone, Deserialize)]
pub struct ControlMessage {
    #[serde(rename = "type")]
    #[serde(default)]
    pub msg_type: ControlType,
    pub seq: u64,
    pub ts: u64,
    pub action: ControlAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlAction {
    Claim,
    Release,
}

impl<'de> Deserialize<'de> for ControlAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        if s.eq_ignore_ascii_case("claim") {
            Ok(Self::Claim)
        } else if s.eq_ignore_ascii_case("release") {
            Ok(Self::Release)
        } else {
            Err(serde::de::Error::custom("invalid control action"))
        }
    }
}

impl Serialize for ControlAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ControlAction::Claim => serializer.serialize_str("claim"),
            ControlAction::Release => serializer.serialize_str("release"),
        }
    }
}

// ============== Game -> Client Messages ==============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WelcomeType {
    #[serde(rename = "welcome")]
    Welcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AckType {
    #[serde(rename = "ack")]
    Ack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AckStatus {
    #[serde(rename = "ok")]
    Ok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorType {
    #[serde(rename = "error")]
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    #[serde(rename = "handshake_required")]
    HandshakeRequired,
    #[serde(rename = "protocol_mismatch")]
    ProtocolMismatch,
    #[serde(rename = "not_controller")]
    NotController,
    #[serde(rename = "controller_active")]
    ControllerActive,
    #[serde(rename = "invalid_command")]
    InvalidCommand,
    #[serde(rename = "invalid_place")]
    InvalidPlace,
    #[serde(rename = "hold_unavailable")]
    HoldUnavailable,
    #[serde(rename = "snapshot_required")]
    SnapshotRequired,
    #[serde(rename = "backpressure")]
    Backpressure,
}

/// Welcome message (response to hello)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WelcomeMessage {
    #[serde(rename = "type")]
    pub msg_type: WelcomeType,
    pub seq: u64,
    pub ts: u64,
    pub protocol_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<AssignedRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controller_id: Option<u64>,
    pub game_id: String,
    pub capabilities: ServerCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub formats: [CapabilityFormat; 1],
    #[serde(rename = "command_modes")]
    pub command_modes: [CapabilityCommandMode; 2],

    /// Feature flags (legacy): union of always-present and optional features.
    pub features: Vec<CapabilityFeature>,

    /// Features that are guaranteed to be present in every observation payload.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "features_always")]
    pub features_always: Vec<CapabilityFeature>,

    /// Features that may be omitted when unknown/not-applicable.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "features_optional")]
    pub features_optional: Vec<CapabilityFeature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityFormat {
    #[serde(rename = "json")]
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityCommandMode {
    #[serde(rename = "action")]
    Action,
    #[serde(rename = "place")]
    Place,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityFeature {
    #[serde(rename = "hold")]
    Hold,
    #[serde(rename = "next")]
    Next,
    #[serde(rename = "next_queue")]
    NextQueue,
    #[serde(rename = "can_hold")]
    CanHold,
    #[serde(rename = "ghost_y")]
    GhostY,
    #[serde(rename = "board_id")]
    BoardId,
    #[serde(rename = "last_event")]
    LastEvent,
    #[serde(rename = "state_hash")]
    StateHash,
    #[serde(rename = "score")]
    Score,
    #[serde(rename = "timers")]
    Timers,
}

/// Acknowledgment for command receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckMessage {
    #[serde(rename = "type")]
    pub msg_type: AckType,
    pub seq: u64,
    pub ts: u64,
    pub status: AckStatus,
}

/// Error message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    #[serde(rename = "type")]
    pub msg_type: ErrorType,
    pub seq: u64,
    pub ts: u64,
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationType {
    #[serde(rename = "observation")]
    Observation,
}

/// Game state observation (sent to all clients)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationMessage {
    #[serde(rename = "type")]
    pub msg_type: ObservationType,
    pub seq: u64,
    pub ts: u64,
    pub playable: bool,
    pub paused: bool,
    #[serde(rename = "game_over")]
    pub game_over: bool,
    #[serde(rename = "episode_id")]
    pub episode_id: u32,
    pub seed: u32,
    #[serde(rename = "piece_id")]
    pub piece_id: u32,
    #[serde(rename = "step_in_piece")]
    pub step_in_piece: u32,
    pub board: BoardSnapshot,
    #[serde(rename = "board_id")]
    pub board_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<ActivePieceSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ghost_y")]
    pub ghost_y: Option<i8>,
    pub next: PieceKindLower, // Single next piece (for compatibility)
    #[serde(rename = "next_queue")]
    pub next_queue: [PieceKindLower; 5], // Full next queue
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold: Option<PieceKindLower>,
    #[serde(rename = "can_hold")]
    pub can_hold: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "last_event")]
    pub last_event: Option<LastEvent>,
    #[serde(rename = "state_hash")]
    pub state_hash: StateHash,
    pub score: u32,
    pub level: u32,
    pub lines: u32,
    pub timers: TimersSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub width: u8,
    pub height: u8,
    pub cells: [[u8; 10]; 20], // 0 = empty, 1-7 = piece kind
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePieceSnapshot {
    pub kind: PieceKindLower,
    pub rotation: RotationLower,
    pub x: i8,
    pub y: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PieceKindLower {
    #[serde(rename = "i")]
    I,
    #[serde(rename = "o")]
    O,
    #[serde(rename = "t")]
    T,
    #[serde(rename = "s")]
    S,
    #[serde(rename = "z")]
    Z,
    #[serde(rename = "j")]
    J,
    #[serde(rename = "l")]
    L,
}

impl From<PieceKind> for PieceKindLower {
    fn from(value: PieceKind) -> Self {
        match value {
            PieceKind::I => Self::I,
            PieceKind::O => Self::O,
            PieceKind::T => Self::T,
            PieceKind::S => Self::S,
            PieceKind::Z => Self::Z,
            PieceKind::J => Self::J,
            PieceKind::L => Self::L,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum RotationLower {
    #[serde(rename = "north")]
    North,
    #[serde(rename = "east")]
    East,
    #[serde(rename = "south")]
    South,
    #[serde(rename = "west")]
    West,
}

impl<'de> Deserialize<'de> for RotationLower {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        if s.eq_ignore_ascii_case("north") {
            Ok(Self::North)
        } else if s.eq_ignore_ascii_case("east") {
            Ok(Self::East)
        } else if s.eq_ignore_ascii_case("south") {
            Ok(Self::South)
        } else if s.eq_ignore_ascii_case("west") {
            Ok(Self::West)
        } else {
            Err(serde::de::Error::custom("invalid rotation"))
        }
    }
}

impl From<Rotation> for RotationLower {
    fn from(value: Rotation) -> Self {
        match value {
            Rotation::North => Self::North,
            Rotation::East => Self::East,
            Rotation::South => Self::South,
            Rotation::West => Self::West,
        }
    }
}

impl From<RotationLower> for Rotation {
    fn from(value: RotationLower) -> Self {
        match value {
            RotationLower::North => Rotation::North,
            RotationLower::East => Rotation::East,
            RotationLower::South => Rotation::South,
            RotationLower::West => Rotation::West,
        }
    }
}

/// Deterministic state hash serialized as lowercase hex (without heap allocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StateHash(pub u64);

impl Serialize for StateHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut buf = [0u8; 16];
        let mut v = self.0;
        for i in 0..16 {
            let nib = (v & 0x0f) as usize;
            buf[15 - i] = HEX[nib];
            v >>= 4;
        }
        let s = std::str::from_utf8(&buf).expect("hex is valid utf8");
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for StateHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        let s = s.trim();
        let mut v: u64 = 0;
        for b in s.as_bytes() {
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u64,
                b'a'..=b'f' => (b - b'a' + 10) as u64,
                b'A'..=b'F' => (b - b'A' + 10) as u64,
                _ => return Err(serde::de::Error::custom("invalid hex")),
            };
            v = (v << 4) | d;
        }
        Ok(StateHash(v))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastEvent {
    pub locked: bool,
    #[serde(rename = "lines_cleared")]
    pub lines_cleared: u32,
    #[serde(rename = "line_clear_score")]
    pub line_clear_score: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tspin: Option<TSpinLower>,
    pub combo: i32,
    #[serde(rename = "back_to_back")]
    pub back_to_back: bool,
}

impl From<CoreLastEvent> for LastEvent {
    fn from(value: CoreLastEvent) -> Self {
        Self {
            locked: value.locked,
            lines_cleared: value.lines_cleared,
            line_clear_score: value.line_clear_score,
            tspin: value.tspin.and_then(|t| match t {
                TSpinKind::Mini => Some(TSpinLower::Mini),
                TSpinKind::Full => Some(TSpinLower::Full),
                TSpinKind::None => None,
            }),
            combo: value.combo,
            back_to_back: value.back_to_back,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TSpinLower {
    #[serde(rename = "mini")]
    Mini,
    #[serde(rename = "full")]
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimersSnapshot {
    #[serde(rename = "drop_ms")]
    pub drop_ms: u32,
    #[serde(rename = "lock_ms")]
    pub lock_ms: u32,
    #[serde(rename = "line_clear_ms")]
    pub line_clear_ms: u32,
}

// ============== Message Parsing ==============

/// Parse a JSON message from a string
pub fn parse_message(json: &str) -> Result<ParsedMessage, serde_json::Error> {
    #[derive(Debug, Deserialize)]
    #[serde(tag = "type")]
    enum InboundMessage {
        #[serde(rename = "hello")]
        Hello(HelloMessage),
        #[serde(rename = "command")]
        Command(CommandMessage),
        #[serde(rename = "control")]
        Control(ControlMessage),
    }

    match serde_json::from_str::<InboundMessage>(json) {
        Ok(InboundMessage::Hello(m)) => Ok(ParsedMessage::Hello(m)),
        Ok(InboundMessage::Command(m)) => Ok(ParsedMessage::Command(m)),
        Ok(InboundMessage::Control(m)) => Ok(ParsedMessage::Control(m)),
        Err(e) => {
            // Unknown message type is not a hard parse error for the protocol.
            #[derive(Debug, Deserialize)]
            struct TypeOnly<'a> {
                #[serde(rename = "type")]
                msg_type: Option<&'a str>,
            }
            let msg_type = serde_json::from_str::<TypeOnly>(json)?
                .msg_type
                .unwrap_or("unknown");
            if msg_type != "hello" && msg_type != "command" && msg_type != "control" {
                #[derive(Debug, Deserialize)]
                struct SeqOnly {
                    seq: Option<u64>,
                }
                let seq = serde_json::from_str::<SeqOnly>(json)?.seq.unwrap_or(0);
                return Ok(ParsedMessage::Unknown(UnknownMessage { seq }));
            }
            Err(e)
        }
    }
}

/// Parsed incoming message
#[derive(Debug, Clone)]
pub enum ParsedMessage {
    Hello(HelloMessage),
    Command(CommandMessage),
    Control(ControlMessage),
    Unknown(UnknownMessage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownMessage {
    pub seq: u64,
}

// ============== Utility Functions ==============

/// Create a hello message
pub fn create_hello(seq: u64, client_name: &str, protocol_version: &str) -> HelloMessage {
    HelloMessage {
        msg_type: HelloType::Hello,
        seq,
        ts: current_timestamp_ms(),
        client: ClientInfo {
            name: client_name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        protocol_version: protocol_version.to_string(),
        formats: FormatsList { json: true },
        requested: RequestedCapabilities {
            stream_observations: true,
            command_mode: CommandMode::Action,
            role: Some(RequestedRole::Auto),
        },
    }
}

/// Create a welcome message
pub fn create_welcome(
    seq: u64,
    protocol_version: &str,
    client_id: u64,
    role: AssignedRole,
    controller_id: Option<u64>,
) -> WelcomeMessage {
    WelcomeMessage {
        msg_type: WelcomeType::Welcome,
        seq,
        ts: current_timestamp_ms(),
        protocol_version: protocol_version.to_string(),
        client_id: Some(client_id),
        role: Some(role),
        controller_id,
        game_id: "tui-tetris".to_string(),
        capabilities: ServerCapabilities {
            formats: [CapabilityFormat::Json],
            command_modes: [CapabilityCommandMode::Action, CapabilityCommandMode::Place],
            features: vec![
                CapabilityFeature::Hold,
                CapabilityFeature::Next,
                CapabilityFeature::NextQueue,
                CapabilityFeature::CanHold,
                CapabilityFeature::GhostY,
                CapabilityFeature::BoardId,
                CapabilityFeature::LastEvent,
                CapabilityFeature::StateHash,
                CapabilityFeature::Score,
                CapabilityFeature::Timers,
            ],

            features_always: vec![
                CapabilityFeature::Next,
                CapabilityFeature::NextQueue,
                CapabilityFeature::CanHold,
                CapabilityFeature::BoardId,
                CapabilityFeature::StateHash,
                CapabilityFeature::Score,
                CapabilityFeature::Timers,
            ],
            features_optional: vec![
                CapabilityFeature::Hold,
                CapabilityFeature::GhostY,
                CapabilityFeature::LastEvent,
            ],
        },
    }
}

/// Create an acknowledgment
pub fn create_ack(seq: u64, _command_seq: u64) -> AckMessage {
    AckMessage {
        msg_type: AckType::Ack,
        seq,
        ts: current_timestamp_ms(),
        status: AckStatus::Ok,
    }
}

/// Create an error message
pub fn create_error(seq: u64, code: ErrorCode, message: &str) -> ErrorMessage {
    ErrorMessage {
        msg_type: ErrorType::Error,
        seq,
        ts: current_timestamp_ms(),
        code,
        message: message.to_string(),
    }
}

/// Get current timestamp in milliseconds
fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CoreLastEvent;

    #[test]
    fn test_parse_hello() {
        let json = r#"{"type":"hello","seq":1,"ts":1234567890,"client":{"name":"test-ai","version":"1.0.0"},"protocol_version":"2.0.0","formats":["json"],"requested":{"stream_observations":true,"command_mode":"action"}}"#;

        let result = parse_message(json).unwrap();
        match result {
            ParsedMessage::Hello(msg) => {
                assert_eq!(msg.msg_type, HelloType::Hello);
                assert_eq!(msg.seq, 1);
                assert_eq!(msg.client.name, "test-ai");
                assert_eq!(msg.protocol_version, "2.0.0");
            }
            _ => panic!("Expected Hello message"),
        }
    }

    #[test]
    fn test_parse_command_action() {
        let json = r#"{"type":"command","seq":2,"ts":1234567900,"mode":"action","actions":["moveLeft","rotateCw","hardDrop"]}"#;

        let result = parse_message(json).unwrap();
        match result {
            ParsedMessage::Command(msg) => {
                assert_eq!(msg.mode, CommandMode::Action);
                let a = msg.actions.unwrap();
                assert_eq!(a.0.len(), 3);
                assert_eq!(a.0[0], ActionName::MoveLeft);
                assert_eq!(a.0[1], ActionName::RotateCw);
                assert_eq!(a.0[2], ActionName::HardDrop);
            }
            _ => panic!("Expected Command message"),
        }
    }

    #[test]
    fn test_parse_control() {
        let json = r#"{"type":"control","seq":3,"ts":1234567910,"action":"claim"}"#;

        let result = parse_message(json).unwrap();
        match result {
            ParsedMessage::Control(msg) => {
                assert_eq!(msg.action, ControlAction::Claim);
            }
            _ => panic!("Expected Control message"),
        }
    }

    #[test]
    fn test_create_welcome() {
        let welcome = create_welcome(1, "2.0.0", 7, AssignedRole::Controller, Some(7));
        assert_eq!(welcome.msg_type, WelcomeType::Welcome);
        assert_eq!(welcome.seq, 1);
        assert_eq!(welcome.protocol_version, "2.0.0");
        assert_eq!(welcome.client_id, Some(7));
        assert_eq!(welcome.role, Some(AssignedRole::Controller));
        assert_eq!(welcome.controller_id, Some(7));
        assert_eq!(welcome.game_id, "tui-tetris");
    }

    #[test]
    fn test_create_error() {
        let error = create_error(
            5,
            ErrorCode::NotController,
            "Only controller may send commands",
        );
        assert_eq!(error.msg_type, ErrorType::Error);
        assert_eq!(error.code, ErrorCode::NotController);
    }

    #[test]
    fn test_serde_roundtrip() {
        let ack = create_ack(10, 5);
        let json = serde_json::to_string(&ack).unwrap();
        let parsed: AckMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seq, ack.seq);
        assert_eq!(parsed.status, ack.status);
    }

    #[test]
    fn test_last_event_from_core_event_maps_tspin_and_combo() {
        let ev = CoreLastEvent {
            locked: true,
            lines_cleared: 2,
            line_clear_score: 1200,
            tspin: Some(crate::types::TSpinKind::Full),
            combo: 1,
            back_to_back: true,
        };

        let mapped = LastEvent::from(ev);
        assert!(mapped.locked);
        assert_eq!(mapped.lines_cleared, 2);
        assert_eq!(mapped.line_clear_score, 1200);
        assert_eq!(mapped.tspin, Some(TSpinLower::Full));
        assert_eq!(mapped.combo, 1);
        assert!(mapped.back_to_back);

        let ev = CoreLastEvent {
            locked: true,
            lines_cleared: 1,
            line_clear_score: 200,
            tspin: Some(crate::types::TSpinKind::Mini),
            combo: 0,
            back_to_back: false,
        };
        let mapped = LastEvent::from(ev);
        assert_eq!(mapped.tspin, Some(TSpinLower::Mini));
        assert_eq!(mapped.combo, 0);
        assert!(!mapped.back_to_back);
    }
}
