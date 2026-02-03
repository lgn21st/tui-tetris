//! Core types module - shared data structures and constants
//!
//! This module defines the fundamental types used throughout the application.
//! All types are pure data structures with no external dependencies, making them
//! usable in any context (core logic, UI rendering, AI protocol).
//!
//! # Board Dimensions
//!
//! Standard Tetris playfield dimensions:
//!
//! - **Width**: 10 columns (indexed 0-9)
//! - **Height**: 20 rows (indexed 0-19)
//! - **Spawn position**: (3, 0) for most pieces
//!
//! # Game Timing Constants
//!
//! Timing values are in milliseconds:
//!
//! | Constant | Value | Description |
//! |----------|-------|-------------|
//! | `TICK_MS` | 16 | Fixed timestep interval (~60 FPS) |
//! | `BASE_DROP_MS` | 1000 | Gravity at level 0 |
//! | `SOFT_DROP_MULTIPLIER` | 10 | Soft drop is 10x faster |
//! | `SOFT_DROP_GRACE_MS` | 150 | Soft drop state timeout |
//! | `LOCK_DELAY_MS` | 450 | Time before piece locks when grounded |
//! | `LOCK_RESET_LIMIT` | 15 | Max lock timer resets per piece |
//! | `LINE_CLEAR_PAUSE_MS` | 180 | Pause duration after line clear |
//! | `LANDING_FLASH_MS` | 120 | Flash duration on piece landing |
//!
//! # DAS/ARR Timing
//!
//! Delayed Auto Shift / Auto Repeat Rate (Tetris Guideline standard):
//!
//! - `DEFAULT_DAS_MS`: 150ms - time before auto-repeat starts
//! - `DEFAULT_ARR_MS`: 50ms - interval between auto-repeats
//! - `SOFT_DROP_ARR_MS`: 50ms - same as ARR for consistency
//!
//! # Drop Intervals by Level
//!
//! Gravity increases with level (milliseconds per row):
//!
//! | Level | Interval |
//! |-------|----------|
//! | 0 | 1000ms |
//! | 1 | 800ms |
//! | 2 | 650ms |
//! | 3 | 500ms |
//! | 4 | 400ms |
//! | 5 | 320ms |
//! | 6 | 250ms |
//! | 7 | 200ms |
//! | 8+ | 160ms (floor at 120ms minimum, 100ms absolute minimum) |
//!
//! # Examples
//!
//! ```
//! use tui_tetris_types::{PieceKind, Rotation, GameAction, BOARD_WIDTH, BOARD_HEIGHT};
//!
//! // Create a piece kind
//! let piece = PieceKind::T;
//!
//! // Parse from string (case-insensitive)
//! let parsed = PieceKind::from_str("t").unwrap();
//! assert_eq!(piece, parsed);
//!
//! // Rotate
//! let rotation = Rotation::North;
//! let rotated = rotation.rotate_cw();
//! assert_eq!(rotated, Rotation::East);
//!
//! // Parse game action
//! let action = GameAction::from_str("moveLeft").unwrap();
//! assert_eq!(action, GameAction::MoveLeft);
//!
//! // Board dimensions
//! assert_eq!(BOARD_WIDTH, 10);
//! assert_eq!(BOARD_HEIGHT, 20);
//! ```

/// Board width in cells (10 columns)
pub const BOARD_WIDTH: u8 = 10;

/// Board height in cells (20 rows)
pub const BOARD_HEIGHT: u8 = 20;

/// Fixed timestep interval in milliseconds (16ms ≈ 60 FPS)
pub const TICK_MS: u32 = 16;

/// Base gravity interval at level 0 (1000ms = 1 second per row)
pub const BASE_DROP_MS: u32 = 1000;

/// Soft drop speed multiplier (10x normal speed) (swiftui-tetris parity).
pub const SOFT_DROP_MULTIPLIER: u32 = 10;

/// Soft drop state timeout (swiftui-tetris parity).
pub const SOFT_DROP_GRACE_MS: u32 = 150;

/// Lock delay when piece is grounded (450ms) (swiftui-tetris parity).
pub const LOCK_DELAY_MS: u32 = 450;

/// Maximum number of lock timer resets per piece (15)
pub const LOCK_RESET_LIMIT: u8 = 15;

/// Pause duration after clearing lines (180ms)
pub const LINE_CLEAR_PAUSE_MS: u32 = 180;

/// Flash duration when piece lands (120ms)
pub const LANDING_FLASH_MS: u32 = 120;

/// DAS (Delayed Auto Shift) delay in milliseconds (swiftui-tetris parity).
pub const DEFAULT_DAS_MS: u32 = 150;

/// ARR (Auto Repeat Rate) in milliseconds (swiftui-tetris parity).
pub const DEFAULT_ARR_MS: u32 = 50;

/// Soft drop DAS in milliseconds (swiftui-tetris parity).
pub const SOFT_DROP_DAS_MS: u32 = 0;

/// Soft drop ARR in milliseconds (swiftui-tetris parity).
pub const SOFT_DROP_ARR_MS: u32 = 50;

/// Drop intervals by level (milliseconds per row)
///
/// Index 0 = Level 0, Index 8 = Level 8+
pub const DROP_INTERVALS: [u32; 9] = [1000, 800, 650, 500, 400, 320, 250, 200, 160];

/// Minimum drop interval floor (120ms)
pub const DROP_INTERVAL_FLOOR_MS: u32 = 120;

/// Absolute minimum drop interval (100ms)
pub const DROP_INTERVAL_MIN_MS: u32 = 100;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swiftui_tetris_parity_timing_defaults() {
        // Source-of-truth: swiftui-tetris/docs/rules-spec.md
        assert_eq!(SOFT_DROP_MULTIPLIER, 10);
        assert_eq!(SOFT_DROP_GRACE_MS, 150);
        assert_eq!(LOCK_DELAY_MS, 450);
        assert_eq!(LOCK_RESET_LIMIT, 15);
        assert_eq!(LINE_CLEAR_PAUSE_MS, 180);
        assert_eq!(LANDING_FLASH_MS, 120);

        assert_eq!(DEFAULT_DAS_MS, 150);
        assert_eq!(DEFAULT_ARR_MS, 50);
        assert_eq!(SOFT_DROP_DAS_MS, 0);
        assert_eq!(SOFT_DROP_ARR_MS, 50);
    }
}

/// The seven tetromino piece kinds
///
/// Each piece has a distinct shape and color:
/// - **I**: Cyan, horizontal bar
/// - **O**: Yellow, 2x2 square
/// - **T**: Magenta, T-shaped
/// - **S**: Green, S-shaped
/// - **Z**: Red, Z-shaped (mirror of S)
/// - **J**: Blue, J-shaped
/// - **L**: Orange, L-shaped (mirror of J)
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
    ///
    /// # Examples
    ///
    /// ```
    /// use tui_tetris_types::PieceKind;
    ///
    /// assert_eq!(PieceKind::from_str("i"), Some(PieceKind::I));
    /// assert_eq!(PieceKind::from_str("O"), Some(PieceKind::O));
    /// assert_eq!(PieceKind::from_str("T"), Some(PieceKind::T));
    /// assert_eq!(PieceKind::from_str("unknown"), None);
    /// ```
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

    /// Convert to lowercase string representation
    ///
    /// # Examples
    ///
    /// ```
    /// use tui_tetris_types::PieceKind;
    ///
    /// assert_eq!(PieceKind::I.as_str(), "i");
    /// assert_eq!(PieceKind::O.as_str(), "o");
    /// ```
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

/// Rotation states following the Super Rotation System (SRS)
///
/// - **North**: Spawn orientation (0° rotation)
/// - **East**: Rotated 90° clockwise
/// - **South**: Rotated 180°
/// - **West**: Rotated 90° counter-clockwise (270° clockwise)
///
/// The rotation cycle goes: North → East → South → West → North
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rotation {
    North,
    East,
    South,
    West,
}

impl Rotation {
    /// Rotate clockwise (90°)
    ///
    /// # Examples
    ///
    /// ```
    /// use tui_tetris_types::Rotation;
    ///
    /// assert_eq!(Rotation::North.rotate_cw(), Rotation::East);
    /// assert_eq!(Rotation::East.rotate_cw(), Rotation::South);
    /// assert_eq!(Rotation::South.rotate_cw(), Rotation::West);
    /// assert_eq!(Rotation::West.rotate_cw(), Rotation::North);
    /// ```
    pub fn rotate_cw(&self) -> Self {
        match self {
            Rotation::North => Rotation::East,
            Rotation::East => Rotation::South,
            Rotation::South => Rotation::West,
            Rotation::West => Rotation::North,
        }
    }

    /// Rotate counter-clockwise (-90° or 270°)
    ///
    /// # Examples
    ///
    /// ```
    /// use tui_tetris_types::Rotation;
    ///
    /// assert_eq!(Rotation::North.rotate_ccw(), Rotation::West);
    /// assert_eq!(Rotation::West.rotate_ccw(), Rotation::South);
    /// assert_eq!(Rotation::South.rotate_ccw(), Rotation::East);
    /// assert_eq!(Rotation::East.rotate_ccw(), Rotation::North);
    /// ```
    pub fn rotate_ccw(&self) -> Self {
        match self {
            Rotation::North => Rotation::West,
            Rotation::West => Rotation::South,
            Rotation::South => Rotation::East,
            Rotation::East => Rotation::North,
        }
    }

    /// Parse rotation from string
    ///
    /// Accepts full names or single letters (case-insensitive):
    /// "north" | "n", "east" | "e", "south" | "s", "west" | "w"
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "north" | "n" => Some(Rotation::North),
            "east" | "e" => Some(Rotation::East),
            "south" | "s" => Some(Rotation::South),
            "west" | "w" => Some(Rotation::West),
            _ => None,
        }
    }

    /// Convert to lowercase string
    pub fn as_str(&self) -> &'static str {
        match self {
            Rotation::North => "north",
            Rotation::East => "east",
            Rotation::South => "south",
            Rotation::West => "west",
        }
    }
}

/// Game actions that can be applied to modify game state
///
/// These actions are used by both human input and AI control.
/// Each action maps to a specific game mechanic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAction {
    /// Move piece one cell left
    MoveLeft,
    /// Move piece one cell right
    MoveRight,
    /// Drop piece one cell down (with soft drop scoring)
    SoftDrop,
    /// Instantly drop piece to lowest valid position
    HardDrop,
    /// Rotate piece 90° clockwise
    RotateCw,
    /// Rotate piece 90° counter-clockwise
    RotateCcw,
    /// Hold current piece (if available)
    Hold,
    /// Toggle pause state
    Pause,
    /// Restart the game (when game over or at any time)
    Restart,
}

impl GameAction {
    /// Parse action from string (for AI protocol)
    ///
    /// # Examples
    ///
    /// ```
    /// use tui_tetris_types::GameAction;
    ///
    /// assert_eq!(GameAction::from_str("moveLeft"), Some(GameAction::MoveLeft));
    /// assert_eq!(GameAction::from_str("rotateCw"), Some(GameAction::RotateCw));
    /// assert_eq!(GameAction::from_str("hardDrop"), Some(GameAction::HardDrop));
    /// assert_eq!(GameAction::from_str("unknown"), None);
    /// ```
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

    /// Convert to camelCase string for AI protocol
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
///
/// T-Spins are detected based on corner occupancy around the T piece.
/// - **None**: Not a T-spin
/// - **Mini**: T-spin with only 3 corners filled or front corners not both filled
/// - **Full**: T-spin with 3+ corners filled and both front corners filled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TSpinKind {
    None,
    Mini,
    Full,
}

/// Core-side event emitted after a piece locks.
///
/// This is engine-internal and can be mapped to adapter protocol `last_event`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreLastEvent {
    pub locked: bool,
    pub lines_cleared: u32,
    pub line_clear_score: u32,
    pub tspin: Option<TSpinKind>,
    pub combo: i32,
    pub back_to_back: bool,
}

impl TSpinKind {
    /// Convert to optional string representation
    ///
    /// Returns `None` for `TSpinKind::None`, `Some("mini")` for Mini,
    /// and `Some("full")` for Full.
    pub fn as_str(&self) -> Option<&'static str> {
        match self {
            TSpinKind::None => None,
            TSpinKind::Mini => Some("mini"),
            TSpinKind::Full => Some("full"),
        }
    }
}

/// A cell on the game board
///
/// - `None`: Empty cell
/// - `Some(PieceKind)`: Cell filled with the specified piece kind
///
/// Used internally by the board as a flat array of cells.
pub type Cell = Option<PieceKind>;

/// Line clear scoring table (Classic Nintendo scoring)
///
/// Base points for clearing N lines at level 0:
/// - 0 lines: 0 points
/// - 1 line: 40 points
/// - 2 lines: 100 points
/// - 3 lines: 300 points
/// - 4 lines: 1200 points (Tetris!)
///
/// Points are multiplied by (level + 1) for higher levels.
pub const LINE_SCORES: [u32; 5] = [0, 40, 100, 300, 1200];

/// Combo scoring base value (50 points per combo step)
///
/// Combo bonus is added after the base line-clear score (and after any B2B multiplier).
pub const COMBO_BASE: u32 = 50;

/// Back-to-back bonus numerator (3/2 = 1.5x multiplier)
///
/// Back-to-back bonuses apply to consecutive Tetrises (4 lines) or T-Spin line clears.
pub const B2B_NUMERATOR: u32 = 3;

/// Back-to-back bonus denominator
pub const B2B_DENOMINATOR: u32 = 2;
