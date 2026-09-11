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
//! | `LOCK_DELAY_MS` | 500 | Time before piece locks when grounded |
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
//! Gravity follows the Guideline formula
//! `(0.8 - (level - 1) * 0.007) ^ (level - 1)` seconds per row, rounded to
//! milliseconds, and is capped at [`DROP_INTERVAL_FLOOR_MS`] (milliseconds per
//! row):
//!
//! | Level | Interval |
//! |-------|----------|
//! | 0 | 1000ms |
//! | 4 | 355ms |
//! | 8 | 94ms |
//! | 12 | 18ms |
//! | 14 | 7ms |
//! | 15+ | 7ms (2.4G, the normal-mode ceiling) |
//!
//! # Examples
//!
//! ```
//! use tetris_core::types::{PieceKind, Rotation, GameAction, BOARD_WIDTH, BOARD_HEIGHT};
//!
//! // Create a piece kind
//! let piece = PieceKind::T;
//!
//! // Parse from string (case-insensitive)
//! let parsed = "t".parse::<PieceKind>().unwrap();
//! assert_eq!(piece, parsed);
//!
//! // Rotate
//! let rotation = Rotation::North;
//! let rotated = rotation.rotate_cw();
//! assert_eq!(rotated, Rotation::East);
//!
//! // Parse game action
//! let action = "moveLeft".parse::<GameAction>().unwrap();
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

/// Soft drop speed multiplier (10x normal speed).
pub const SOFT_DROP_MULTIPLIER: u32 = 10;

/// Soft drop state timeout.
pub const SOFT_DROP_GRACE_MS: u32 = 150;

/// Lock delay when piece is grounded (500ms).
pub const LOCK_DELAY_MS: u32 = 500;

/// Maximum number of lock timer resets per piece (15)
pub const LOCK_RESET_LIMIT: u8 = 15;

/// Pause duration after clearing lines (180ms)
pub const LINE_CLEAR_PAUSE_MS: u32 = 180;

/// Flash duration when piece lands (120ms)
pub const LANDING_FLASH_MS: u32 = 120;

/// DAS (Delayed Auto Shift) delay in milliseconds.
pub const DEFAULT_DAS_MS: u32 = 150;

/// ARR (Auto Repeat Rate) in milliseconds.
pub const DEFAULT_ARR_MS: u32 = 50;

/// Soft drop DAS in milliseconds.
pub const SOFT_DROP_DAS_MS: u32 = 0;

/// Soft drop ARR in milliseconds.
pub const SOFT_DROP_ARR_MS: u32 = 50;

/// Drop intervals by engine level (milliseconds per row).
///
/// Derived from the Guideline gravity formula
/// `(0.8 - (level - 1) * 0.007) ^ (level - 1)` seconds per row, rounded to
/// whole milliseconds. Index 0 = Level 0 … index 14 = Level 14; level 15+ uses
/// [`DROP_INTERVAL_FLOOR_MS`].
pub const DROP_INTERVALS: [u32; 15] = [
    1000, 793, 618, 473, 355, 262, 190, 135, 94, 64, 43, 28, 18, 11, 7,
];

/// Gravity floor from level 15 (7 ms per row ≈ 2.4G).
///
/// Official games reserve 20G for level 19+ or a separate high-level mode and
/// keep normal modes around speed level 15, so a normal-mode curve stops here.
pub const DROP_INTERVAL_FLOOR_MS: u32 = 7;

/// Version of this local core timing profile.
///
/// The portable ruleset excludes gravity and tick timing
/// (`protocol/rules/SPEC.md` §10), so deterministic trajectories depend on this
/// profile as well. Replay metadata and the adapter welcome record it together
/// with the portable ruleset id.
pub const TIMING_PROFILE_ID: &str = "tui-tetris-timing-1.1.0";

/// Maximum ordered lock/line-clear events retained for one core transition.
pub const MAX_CORE_EVENTS: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_defaults_match_rules_spec() {
        // Local timing: docs/rules-spec.md; portable rules: protocol/rules/
        assert_eq!(SOFT_DROP_MULTIPLIER, 10);
        assert_eq!(SOFT_DROP_GRACE_MS, 150);
        assert_eq!(LOCK_DELAY_MS, 500);
        assert_eq!(LINE_SCORES, [0, 100, 300, 500, 800]);
        assert_eq!(LOCK_RESET_LIMIT, 15);
        assert_eq!(LINE_CLEAR_PAUSE_MS, 180);
        assert_eq!(LANDING_FLASH_MS, 120);

        assert_eq!(DEFAULT_DAS_MS, 150);
        assert_eq!(DEFAULT_ARR_MS, 50);
        assert_eq!(SOFT_DROP_DAS_MS, 0);
        assert_eq!(SOFT_DROP_ARR_MS, 50);

        assert_eq!(
            DROP_INTERVALS,
            [
                1000, 793, 618, 473, 355, 262, 190, 135, 94, 64, 43, 28, 18, 11, 7
            ]
        );
        assert_eq!(DROP_INTERVAL_FLOOR_MS, 7);
        assert_eq!(MAX_CORE_EVENTS, 4);
    }

    #[test]
    fn drop_intervals_follow_the_guideline_gravity_formula() {
        // The portable ruleset excludes gravity tables (protocol/rules/SPEC.md
        // §10); this local profile adopts the Guideline curve for the whole
        // table rather than a table that plateaus early.
        for (level, &actual) in DROP_INTERVALS.iter().enumerate() {
            let l = level as f64;
            let seconds = (0.8 - l * 0.007).powf(l);
            let expected = (seconds * 1000.0).round().max(1.0) as u32;
            assert_eq!(actual, expected, "level {level}");
        }

        // The cap binds from level 15, where the formula would already be
        // faster. Official games keep normal modes around this point and reserve
        // 20G for level 19+ or a separate high-level mode.
        let at_15 = (0.8 - 15.0 * 0.007f64).powf(15.0) * 1000.0;
        assert!(at_15 < DROP_INTERVAL_FLOOR_MS as f64);
        assert_eq!(DROP_INTERVAL_FLOOR_MS, DROP_INTERVALS[14]);

        // 7 ms per row is roughly 2.4G: fast, but far short of the 20G endgame
        // that would need Infinity lock delay to stay playable.
        let rows_per_second = 1000.0 / DROP_INTERVAL_FLOOR_MS as f64;
        assert!((2.0..3.0).contains(&(rows_per_second / 60.0)));
    }

    #[test]
    fn protocol_types_parse_case_insensitively_without_allocating_lowercase_strings() {
        assert_eq!("t".parse::<PieceKind>(), Ok(PieceKind::T));
        assert_eq!("WEST".parse::<Rotation>(), Ok(Rotation::West));
        assert_eq!("RotateCcw".parse::<GameAction>(), Ok(GameAction::RotateCcw));
        assert!("unknown".parse::<GameAction>().is_err());
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
    /// Parse a piece kind without allocating. Prefer the standard `str::parse`
    /// interface in new code; this wrapper preserves the original public API.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// Convert to lowercase string representation
    ///
    /// # Examples
    ///
    /// ```
    /// use tetris_core::types::PieceKind;
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

impl std::str::FromStr for PieceKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [
            Self::I,
            Self::O,
            Self::T,
            Self::S,
            Self::Z,
            Self::J,
            Self::L,
        ]
        .into_iter()
        .find(|kind| s.eq_ignore_ascii_case(kind.as_str()))
        .ok_or(())
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
    /// Parse a rotation without allocating. Prefer `str::parse` in new code.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// Rotate clockwise (90°)
    ///
    /// # Examples
    ///
    /// ```
    /// use tetris_core::types::Rotation;
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
    /// use tetris_core::types::Rotation;
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

impl std::str::FromStr for Rotation {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            value if value.eq_ignore_ascii_case("north") || value.eq_ignore_ascii_case("n") => {
                Ok(Self::North)
            }
            value if value.eq_ignore_ascii_case("east") || value.eq_ignore_ascii_case("e") => {
                Ok(Self::East)
            }
            value if value.eq_ignore_ascii_case("south") || value.eq_ignore_ascii_case("s") => {
                Ok(Self::South)
            }
            value if value.eq_ignore_ascii_case("west") || value.eq_ignore_ascii_case("w") => {
                Ok(Self::West)
            }
            _ => Err(()),
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
    /// Parse an action without allocating. Prefer `str::parse` in new code.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        s.parse().ok()
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

impl std::str::FromStr for GameAction {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [
            Self::MoveLeft,
            Self::MoveRight,
            Self::SoftDrop,
            Self::HardDrop,
            Self::RotateCw,
            Self::RotateCcw,
            Self::Hold,
            Self::Pause,
            Self::Restart,
        ]
        .into_iter()
        .find(|action| s.eq_ignore_ascii_case(action.as_str()))
        .ok_or(())
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
/// This is engine-internal and can be mapped to an adapter protocol event.
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

/// Line clear scoring table (Guideline DS/Friends)
///
/// Base points for clearing N lines at guideline level 1 (engine level 0):
/// - 0 lines: 0 points
/// - 1 line: 100 points
/// - 2 lines: 300 points
/// - 3 lines: 500 points
/// - 4 lines: 800 points (Tetris)
///
/// Points are multiplied by guideline level (`engine_level + 1`).
pub const LINE_SCORES: [u32; 5] = [0, 100, 300, 500, 800];

/// Combo scoring base value (50 points per combo step)
///
/// Combo bonus is `COMBO_BASE * combo_index * guideline_level`, added after
/// the base line-clear score (and after any B2B multiplier).
pub const COMBO_BASE: u32 = 50;

/// Back-to-back bonus numerator (3/2 = 1.5x multiplier)
///
/// Back-to-back bonuses apply to consecutive Tetrises or T-Spin line clears
/// (including Mini T-Spin with lines).
pub const B2B_NUMERATOR: u32 = 3;

/// Back-to-back bonus denominator
pub const B2B_DENOMINATOR: u32 = 2;
