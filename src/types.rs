//! Core types shared across the application
//! This module contains pure data types with no external dependencies

/// Board dimensions
pub const BOARD_WIDTH: u8 = 10;
pub const BOARD_HEIGHT: u8 = 20;

/// Game timing constants (in milliseconds)
pub const TICK_MS: u32 = 16;
pub const BASE_DROP_MS: u32 = 1000;
pub const SOFT_DROP_MULTIPLIER: u32 = 10;
pub const SOFT_DROP_GRACE_MS: u32 = 150;
pub const LOCK_DELAY_MS: u32 = 450;
pub const LOCK_RESET_LIMIT: u8 = 15;
pub const LINE_CLEAR_PAUSE_MS: u32 = 180;
pub const LANDING_FLASH_MS: u32 = 120;

/// DAS/ARR timing (milliseconds)
pub const DEFAULT_DAS_MS: u32 = 150;
pub const DEFAULT_ARR_MS: u32 = 50;
pub const SOFT_DROP_ARR_MS: u32 = 50;

/// Drop intervals by level (milliseconds)
pub const DROP_INTERVALS: [u32; 9] = [1000, 800, 650, 500, 400, 320, 250, 200, 160];
pub const DROP_INTERVAL_FLOOR_MS: u32 = 120;
pub const DROP_INTERVAL_MIN_MS: u32 = 100;

/// Tetromino piece kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceKind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl PieceKind {
    /// Parse piece kind from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "i" => Some(PieceKind::I),
            "o" => Some(PieceKind::O),
            "t" => Some(PieceKind::T),
            "s" => Some(PieceKind::S),
            "z" => Some(PieceKind::Z),
            "j" => Some(PieceKind::J),
            "l" => Some(PieceKind::L),
            _ => None,
        }
    }

    /// Convert to lowercase string
    pub fn as_str(&self) -> &'static str {
        match self {
            PieceKind::I => "i",
            PieceKind::O => "o",
            PieceKind::T => "t",
            PieceKind::S => "s",
            PieceKind::Z => "z",
            PieceKind::J => "j",
            PieceKind::L => "l",
        }
    }
}

/// Rotation states (North = spawn orientation)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rotation {
    North,
    East,
    South,
    West,
}

impl Rotation {
    /// Rotate clockwise
    pub fn rotate_cw(&self) -> Self {
        match self {
            Rotation::North => Rotation::East,
            Rotation::East => Rotation::South,
            Rotation::South => Rotation::West,
            Rotation::West => Rotation::North,
        }
    }

    /// Rotate counter-clockwise
    pub fn rotate_ccw(&self) -> Self {
        match self {
            Rotation::North => Rotation::West,
            Rotation::West => Rotation::South,
            Rotation::South => Rotation::East,
            Rotation::East => Rotation::North,
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "north" | "n" => Some(Rotation::North),
            "east" | "e" => Some(Rotation::East),
            "south" | "s" => Some(Rotation::South),
            "west" | "w" => Some(Rotation::West),
            _ => None,
        }
    }

    /// Convert to string
    pub fn as_str(&self) -> &'static str {
        match self {
            Rotation::North => "north",
            Rotation::East => "east",
            Rotation::South => "south",
            Rotation::West => "west",
        }
    }
}

/// Game actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAction {
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

impl GameAction {
    /// Parse action from string (for AI protocol)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "moveleft" => Some(GameAction::MoveLeft),
            "moveright" => Some(GameAction::MoveRight),
            "softdrop" => Some(GameAction::SoftDrop),
            "harddrop" => Some(GameAction::HardDrop),
            "rotatecw" => Some(GameAction::RotateCw),
            "rotateccw" => Some(GameAction::RotateCcw),
            "hold" => Some(GameAction::Hold),
            "pause" => Some(GameAction::Pause),
            "restart" => Some(GameAction::Restart),
            _ => None,
        }
    }

    /// Convert to string
    pub fn as_str(&self) -> &'static str {
        match self {
            GameAction::MoveLeft => "moveLeft",
            GameAction::MoveRight => "moveRight",
            GameAction::SoftDrop => "softDrop",
            GameAction::HardDrop => "hardDrop",
            GameAction::RotateCw => "rotateCw",
            GameAction::RotateCcw => "rotateCcw",
            GameAction::Hold => "hold",
            GameAction::Pause => "pause",
            GameAction::Restart => "restart",
        }
    }
}

/// T-Spin detection result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TSpinKind {
    None,
    Mini,
    Full,
}

impl TSpinKind {
    pub fn as_str(&self) -> Option<&'static str> {
        match self {
            TSpinKind::None => None,
            TSpinKind::Mini => Some("mini"),
            TSpinKind::Full => Some("full"),
        }
    }
}

/// Cell on the board (None = empty, Some = filled with piece kind)
pub type Cell = Option<PieceKind>;

/// Line clear scoring (Classic rules)
pub const LINE_SCORES: [u32; 5] = [0, 40, 100, 300, 1200];

/// Combo scoring base
pub const COMBO_BASE: u32 = 50;

/// Back-to-back bonus multiplier (as numerator, denominator is 2)
pub const B2B_NUMERATOR: u32 = 3;
pub const B2B_DENOMINATOR: u32 = 2;
