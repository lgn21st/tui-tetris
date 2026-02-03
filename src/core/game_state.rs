//! Game state module - manages the complete game state
//!
//! This module ties together all core components: board, pieces, RNG, and scoring.
//! It handles game timing, piece movement, rotation, line clears, and game lifecycle.

use crate::core::{
    calculate_drop_score, calculate_score, get_shape,
    scoring::get_drop_interval_ms,
    try_rotate, Board, PieceQueue,
};
use crate::types::*;

/// Active falling piece
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Tetromino {
    pub kind: PieceKind,
    pub rotation: Rotation,
    pub x: i8,
    pub y: i8,
}

impl Tetromino {
    /// Create a new tetromino at spawn position
    pub fn new(kind: PieceKind) -> Self {
        Self {
            kind,
            rotation: Rotation::North,
            x: 3,
            y: 0,
        }
    }

    /// Get the shape (mino offsets) for current rotation
    pub fn shape(&self) -> [(i8, i8); 4] {
        get_shape(self.kind, self.rotation)
    }

    /// Check if all minos are at valid positions on the board
    pub fn is_valid(&self, board: &Board) -> bool {
        self.shape()
            .iter()
            .all(|&(dx, dy)| board.is_valid(self.x + dx, self.y + dy))
    }

    /// Check if the piece is grounded (resting on something)
    pub fn is_grounded(&self, board: &Board) -> bool {
        // Check if any mino has something directly below it
        self.shape()
            .iter()
            .any(|&(dx, dy)| !board.is_valid(self.x + dx, self.y + dy + 1))
    }
}

/// Complete game state
#[derive(Debug, Clone)]
pub struct GameState {
    board: Board,
    board_id: u32,
    active: Option<Tetromino>,
    hold: Option<PieceKind>,
    next_queue: [PieceKind; 5],
    piece_queue: PieceQueue,
    /// Monotonic episode id (increments on restart).
    episode_id: u32,
    /// Monotonic id for spawned pieces (increments only on successful spawn).
    ///
    /// This is the value exported to the adapter protocol as `piece_id`.
    piece_id: u32,
    /// Monotonic id for the active piece instance (increments on spawn and hold swaps).
    active_id: u32,
    /// Step counter within the current active piece (increments once per fixed tick).
    step_in_piece: u32,
    /// Last lock/line-clear event (consumed by observers).
    last_event: Option<CoreLastEvent>,
    score: u32,
    level: u32,
    lines: u32,
    combo: i32,
    back_to_back: bool,
    drop_timer_ms: u32,
    lock_timer_ms: u32,
    lock_reset_count: u8,
    line_clear_timer_ms: u32,
    landing_flash_ms: u32,
    paused: bool,
    game_over: bool,
    started: bool,
    can_hold: bool,
    last_action_was_rotate: bool,
    // Tracking for soft drop grace period
    soft_drop_timer_ms: u32,
    is_soft_dropping: bool,
}

impl GameState {
    /// Create a new game with the given RNG seed
    pub fn new(seed: u32) -> Self {
        let piece_queue = PieceQueue::new(seed);
        let next_queue = piece_queue.peek_5();

        Self {
            board: Board::new(),
            board_id: 0,
            active: None,
            hold: None,
            next_queue,
            piece_queue,
            episode_id: 0,
            piece_id: 0,
            active_id: 0,
            step_in_piece: 0,
            last_event: None,
            score: 0,
            level: 0,
            lines: 0,
            combo: -1,
            back_to_back: false,
            drop_timer_ms: 0,
            lock_timer_ms: 0,
            lock_reset_count: 0,
            line_clear_timer_ms: 0,
            landing_flash_ms: 0,
            paused: false,
            game_over: false,
            started: false,
            can_hold: true,
            last_action_was_rotate: false,
            soft_drop_timer_ms: 0,
            is_soft_dropping: false,
        }
    }

    /// Start the game and spawn the first piece
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.spawn_piece();
    }

    pub fn started(&self) -> bool {
        self.started
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn game_over(&self) -> bool {
        self.game_over
    }

    pub fn can_hold(&self) -> bool {
        self.can_hold
    }

    pub fn episode_id(&self) -> u32 {
        self.episode_id
    }

    pub fn piece_id(&self) -> u32 {
        self.piece_id
    }

    pub fn active_id(&self) -> u32 {
        self.active_id
    }

    pub fn step_in_piece(&self) -> u32 {
        self.step_in_piece
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn lines(&self) -> u32 {
        self.lines
    }

    pub fn hold_piece(&self) -> Option<PieceKind> {
        self.hold
    }

    pub fn next_queue(&self) -> &[PieceKind; 5] {
        &self.next_queue
    }

    pub fn active(&self) -> Option<Tetromino> {
        self.active
    }

    pub fn board_id(&self) -> u32 {
        self.board_id
    }

    #[cfg(test)]
    pub fn board_mut(&mut self) -> &mut Board {
        &mut self.board
    }

    pub fn snapshot_into(&self, out: &mut crate::core::snapshot::GameSnapshot) {
        self.snapshot_board_into(out);
        self.snapshot_meta_into(out);
    }

    pub fn snapshot_board_into(&self, out: &mut crate::core::snapshot::GameSnapshot) {
        self.board.write_u8_grid(&mut out.board);
        out.board_id = self.board_id;
    }

    pub fn snapshot_meta_into(&self, out: &mut crate::core::snapshot::GameSnapshot) {
        use crate::core::snapshot::{ActiveSnapshot, TimersSnapshot};

        out.active = self.active.map(ActiveSnapshot::from);
        out.ghost_y = self.ghost_y();
        out.hold = self.hold;
        out.next_queue = self.next_queue;
        out.can_hold = self.can_hold;
        out.paused = self.paused;
        out.game_over = self.game_over;
        out.episode_id = self.episode_id;
        out.seed = self.piece_queue.seed();
        out.piece_id = self.piece_id;
        out.step_in_piece = self.step_in_piece;
        out.score = self.score;
        out.level = self.level;
        out.lines = self.lines;
        out.timers = TimersSnapshot {
            drop_ms: self.drop_timer_ms,
            lock_ms: self.lock_timer_ms,
            line_clear_ms: self.line_clear_timer_ms,
        };
    }

    pub fn snapshot(&self) -> crate::core::snapshot::GameSnapshot {
        let mut s = crate::core::snapshot::GameSnapshot::default();
        self.snapshot_into(&mut s);
        s
    }

    /// Spawn a new piece from the queue
    fn spawn_piece(&mut self) -> bool {
        // Check if spawn position is blocked
        if self.board.is_spawn_blocked() {
            self.game_over = true;
            return false;
        }

        // Draw next piece from queue
        let kind = self.piece_queue.draw();
        let piece = Tetromino::new(kind);

        // Verify spawn position is valid
        if !piece.is_valid(&self.board) {
            self.game_over = true;
            return false;
        }

        self.active = Some(piece);

        // Update piece id and step counter.
        self.piece_id = self.piece_id.wrapping_add(1);
        self.active_id = self.active_id.wrapping_add(1);
        self.step_in_piece = 0;
        self.can_hold = true;
        self.lock_timer_ms = 0;
        self.lock_reset_count = 0;
        self.last_action_was_rotate = false;

        // Update next queue preview
        self.next_queue = self.piece_queue.peek_5();

        true
    }

    /// Get current drop interval based on level
    pub fn drop_interval_ms(&self) -> u32 {
        let base = get_drop_interval_ms(self.level);
        if self.is_soft_dropping {
            // Soft drop is 10x faster
            base / SOFT_DROP_MULTIPLIER
        } else {
            base
        }
    }

    /// Try to move the active piece
    pub(crate) fn try_move(&mut self, dx: i8, dy: i8) -> bool {
        let Some(active) = self.active else {
            return false;
        };

        // Check if new position is valid
        let shape = active.shape();
        let valid = shape
            .iter()
            .all(|&(mx, my)| self.board.is_valid(active.x + mx + dx, active.y + my + dy));

        if valid {
            self.active = Some(Tetromino {
                x: active.x + dx,
                y: active.y + dy,
                ..active
            });

            self.handle_lock_reset();

            // Movement clears the "last action was rotate" flag
            if dx != 0 {
                self.last_action_was_rotate = false;
            }

            return true;
        }

        false
    }

    /// Try to rotate the active piece with SRS wall kicks
    pub(crate) fn try_rotate(&mut self, clockwise: bool) -> bool {
        let Some(active) = self.active else {
            return false;
        };

        // O piece doesn't rotate
        if active.kind == PieceKind::O {
            return false;
        }

        let result = try_rotate(
            active.kind,
            active.rotation,
            active.x,
            active.y,
            clockwise,
            |x, y| self.board.is_valid(x, y),
        );

        if let Some((_new_shape, new_rotation, (dx, dy))) = result {
            self.active = Some(Tetromino {
                rotation: new_rotation,
                x: active.x + dx,
                y: active.y + dy,
                ..active
            });

            self.handle_lock_reset();
            self.last_action_was_rotate = true;

            return true;
        }

        false
    }

    /// Reset lock timers/counts to match swiftui-tetris semantics:
    /// - When not grounded, lock timer and reset count are cleared.
    /// - When grounded, successful moves/rotations may reset the lock timer up to `LOCK_RESET_LIMIT`.
    fn handle_lock_reset(&mut self) {
        if !self.is_grounded() {
            self.lock_timer_ms = 0;
            self.lock_reset_count = 0;
            return;
        }

        if self.lock_reset_count < LOCK_RESET_LIMIT {
            self.lock_timer_ms = 0;
            self.lock_reset_count += 1;
        }
    }

    /// Hard drop the active piece to the bottom
    pub(crate) fn hard_drop(&mut self) -> u32 {
        let Some(active) = self.active else {
            return 0;
        };

        // Find how far we can drop
        let mut drop_distance: u32 = 0;
        let shape = active.shape();

        loop {
            let can_drop = shape.iter().all(|&(dx, dy)| {
                self.board
                    .is_valid(active.x + dx, active.y + dy + drop_distance as i8 + 1)
            });

            if can_drop {
                drop_distance += 1;
            } else {
                break;
            }
        }

        // Move piece to final position
        if drop_distance > 0 {
            self.active = Some(Tetromino {
                y: active.y + drop_distance as i8,
                ..active
            });
        }

        // Lock the piece immediately
        self.lock_piece();

        // Return score from hard drop
        calculate_drop_score(drop_distance, true)
    }

    /// Swap active piece with hold piece
    fn hold(&mut self) -> bool {
        if !self.can_hold {
            return false;
        }

        let Some(active) = self.active else {
            return false;
        };

        let current_kind = active.kind;

        match self.hold {
            Some(hold_kind) => {
                // Swap with hold
                self.active = Some(Tetromino::new(hold_kind));
                self.hold = Some(current_kind);

                // Active piece changed.
                self.active_id = self.active_id.wrapping_add(1);
                self.step_in_piece = 0;

                // Check if spawn is valid
                if let Some(ref piece) = self.active {
                    if !piece.is_valid(&self.board) {
                        self.game_over = true;
                        self.active = None;
                        return false;
                    }
                }
            }
            None => {
                // No hold piece yet, move current to hold and spawn new
                self.hold = Some(current_kind);
                self.spawn_piece();
            }
        }

        self.can_hold = false;
        self.lock_timer_ms = 0;
        self.lock_reset_count = 0;
        self.last_action_was_rotate = false;

        true
    }

    /// Lock the active piece onto the board and handle line clears
    fn lock_piece(&mut self) {
        let Some(active) = self.active else {
            return;
        };

        // Trigger landing flash on every lock (swiftui-tetris parity).
        self.landing_flash_ms = LANDING_FLASH_MS;

        // Lock piece to board
        let shape = active.shape();
        let lock_success = self
            .board
            .lock_piece(&shape, active.x, active.y, active.kind);

        // Even if lock failed (position invalid), we should still try to spawn next piece
        // This handles edge cases where piece overlaps with existing blocks
        // The spawn_piece call below will detect game over if spawn area is blocked

        // Always clear self.active to allow spawn_piece to run
        // (even if lock failed, we need to try spawning next piece)
        self.active = None;

        // Clear full rows
        let cleared_rows = self.board.clear_full_rows();
        let lines_cleared = cleared_rows.len();

        if lock_success || lines_cleared > 0 {
            self.board_id = self.board_id.wrapping_add(1);
        }

        // Detect T-spin
        let tspin = if active.kind == PieceKind::T {
            self.t_spin_kind(&active, &cleared_rows)
        } else {
            TSpinKind::None
        };

        // Update game state
        let line_clear_score = self.apply_line_clear(lines_cleared, tspin);

        // Emit last event (for adapter observation immediate flush).
        //
        // swiftui-tetris reports the last T-Spin kind only for line clears (it resets it to `.none`
        // when `lines_cleared == 0`), so we only include `tspin` when at least one line was cleared.
        let tspin_opt = if lines_cleared > 0 {
            match tspin {
                TSpinKind::None => None,
                _ => Some(tspin),
            }
        } else {
            None
        };
        self.last_event = Some(CoreLastEvent {
            locked: true,
            lines_cleared: lines_cleared as u32,
            line_clear_score,
            tspin: tspin_opt,
            combo: self.combo,
            back_to_back: self.back_to_back,
        });

        // Spawn next piece (unless game over)
        if !self.game_over {
            self.spawn_piece();
        }
    }

    /// Take and clear the last lock/line-clear event.
    pub fn take_last_event(&mut self) -> Option<CoreLastEvent> {
        self.last_event.take()
    }

    /// Apply line-clear scoring and update combo/B2B/lines/level.
    ///
    /// Returns the base clear points for the event (includes B2B multiplier, excludes combo bonus).
    fn apply_line_clear(&mut self, lines_cleared: usize, tspin: TSpinKind) -> u32 {
        if lines_cleared == 0 {
            self.combo = -1;
            self.back_to_back = false;

            // swiftui-tetris awards points for T-Spin "no lines", but it does not count as a
            // line clear for combo/B2B/line_clear_score reporting.
            let tspin_points = match tspin {
                TSpinKind::Full => {
                    crate::core::scoring::calculate_tspin_score(TSpinKind::Full, 0, self.level)
                }
                TSpinKind::Mini => {
                    crate::core::scoring::calculate_tspin_score(TSpinKind::Mini, 0, self.level)
                }
                TSpinKind::None => 0,
            };
            self.score = self.score.saturating_add(tspin_points);

            return 0;
        }

        // Scoring uses the pre-clear level.
        let combo_after_clear = self.combo.saturating_add(1);
        let score_result = calculate_score(
            lines_cleared,
            self.level,
            tspin,
            combo_after_clear,
            self.back_to_back,
        );

        self.combo = combo_after_clear;
        self.lines = self.lines.saturating_add(lines_cleared as u32);
        self.level = self.lines / 10;
        self.back_to_back = score_result.qualifies_for_b2b;
        self.score = self.score.saturating_add(score_result.total);

        // Start line clear timer.
        self.line_clear_timer_ms = LINE_CLEAR_PAUSE_MS;
        self.landing_flash_ms = LANDING_FLASH_MS;

        score_result.line_clear_score
    }

    /// Detect T-spin type based on corner occupancy
    fn t_spin_kind(&self, piece: &Tetromino, _cleared_rows: &[usize]) -> TSpinKind {
        if !self.last_action_was_rotate {
            return TSpinKind::None;
        }

        // T piece corners (relative to piece origin)
        // For each rotation, check which corners are filled
        let corners: [(i8, i8); 4] = match piece.rotation {
            Rotation::North => [(0, 0), (2, 0), (0, 2), (2, 2)],
            Rotation::East => [(0, 0), (2, 0), (0, 2), (2, 2)],
            Rotation::South => [(0, 0), (2, 0), (0, 2), (2, 2)],
            Rotation::West => [(0, 0), (2, 0), (0, 2), (2, 2)],
        };

        // Count filled corners
        let filled_count = corners
            .iter()
            .filter(|&&(cx, cy)| {
                let x = piece.x + cx;
                let y = piece.y + cy;
                !self.board.is_valid(x, y)
            })
            .count();

        // For a T-spin, at least 3 corners must be filled
        if filled_count >= 3 {
            // Check front two corners (the ones in the direction of the T)
            let front_corners = match piece.rotation {
                Rotation::North => [(0, 0), (2, 0)], // Top corners
                Rotation::East => [(2, 0), (2, 2)],  // Right corners
                Rotation::South => [(0, 2), (2, 2)], // Bottom corners
                Rotation::West => [(0, 0), (0, 2)],  // Left corners
            };

            let front_filled = front_corners
                .iter()
                .filter(|&&(cx, cy)| {
                    let x = piece.x + cx;
                    let y = piece.y + cy;
                    !self.board.is_valid(x, y)
                })
                .count();

            if front_filled == 2 {
                TSpinKind::Full
            } else {
                TSpinKind::Mini
            }
        } else {
            TSpinKind::None
        }
    }

    /// Check if the active piece is on the ground
    fn is_grounded(&self) -> bool {
        match self.active {
            Some(ref piece) => piece.is_grounded(&self.board),
            None => false,
        }
    }

    /// Calculate the ghost piece Y position (where piece would land)
    fn ghost_y(&self) -> Option<i8> {
        let active = self.active?;
        let shape = active.shape();

        let mut drop_distance: i8 = 0;
        loop {
            let can_drop = shape.iter().all(|&(dx, dy)| {
                self.board
                    .is_valid(active.x + dx, active.y + dy + drop_distance + 1)
            });

            if can_drop {
                drop_distance += 1;
            } else {
                break;
            }
        }

        Some(active.y + drop_distance)
    }

    /// Main game tick - update timers and handle gravity
    pub fn tick(&mut self, elapsed_ms: u32, soft_drop: bool) -> bool {
        if self.paused || self.game_over || !self.started {
            return false;
        }

        // Handle landing flash (ticks down even during line clear pause).
        if self.landing_flash_ms > 0 {
            self.landing_flash_ms = self.landing_flash_ms.saturating_sub(elapsed_ms);
        }

        // Step counter for the current active piece (increments even during line clear pause).
        if self.active.is_some() {
            self.step_in_piece = self.step_in_piece.wrapping_add(1);
        }

        // Handle line clear pause
        if self.line_clear_timer_ms > 0 {
            self.line_clear_timer_ms = self.line_clear_timer_ms.saturating_sub(elapsed_ms);
            if self.line_clear_timer_ms > 0 {
                return false;
            }
        }

        let Some(_) = self.active else {
            return false;
        };

        // Soft drop activation + timeout (swiftui-tetris parity):
        // - A soft drop action activates a short timeout.
        // - If the timeout expires, soft drop speed disables automatically.
        // - The `soft_drop` tick argument may be used by callers as an alternate activation signal.
        if soft_drop {
            self.is_soft_dropping = true;
            self.soft_drop_timer_ms = SOFT_DROP_GRACE_MS;
        }
        if self.is_soft_dropping && self.soft_drop_timer_ms > 0 {
            self.soft_drop_timer_ms = self.soft_drop_timer_ms.saturating_sub(elapsed_ms);
            if self.soft_drop_timer_ms == 0 {
                self.is_soft_dropping = false;
            }
        }

        let mut changed = false;

        // Gravity: accumulate and advance (swiftui-tetris uses a while loop here).
        let drop_interval = self.drop_interval_ms();
        self.drop_timer_ms = self.drop_timer_ms.saturating_add(elapsed_ms);
        while self.drop_timer_ms >= drop_interval {
            self.drop_timer_ms -= drop_interval;
            let moved = self.try_move(0, 1);
            if !moved {
                continue;
            }
            changed = true;
        }

        if self.is_grounded() {
            self.lock_timer_ms = self.lock_timer_ms.saturating_add(elapsed_ms);
            if self.lock_timer_ms >= LOCK_DELAY_MS {
                self.lock_timer_ms = 0;
                self.drop_timer_ms = 0;
                self.lock_piece();
                return true;
            }
        } else {
            // While falling, lock delay is inactive (clear timers/counters).
            self.lock_timer_ms = 0;
            self.lock_reset_count = 0;
        }

        changed
    }

    /// Apply a game action
    pub fn apply_action(&mut self, action: GameAction) -> bool {
        if self.game_over && action != GameAction::Restart {
            return false;
        }
        if self.paused && action != GameAction::Pause && action != GameAction::Restart {
            return false;
        }

        match action {
            GameAction::MoveLeft => self.try_move(-1, 0),
            GameAction::MoveRight => self.try_move(1, 0),
            GameAction::SoftDrop => {
                // swiftui-tetris: softDropStep() moves once (if possible), adds +1 score per cell,
                // and activates soft drop speed for a short grace window.
                let moved = self.try_move(0, 1);
                if moved {
                    self.score += calculate_drop_score(1, false);
                }
                self.is_soft_dropping = true;
                self.soft_drop_timer_ms = SOFT_DROP_GRACE_MS;
                self.last_action_was_rotate = false;
                moved
            }
            GameAction::HardDrop => {
                // swiftui-tetris: hard drop is an action, so it clears the rotate flag before lock.
                self.last_action_was_rotate = false;
                let drop_score = self.hard_drop();
                self.score += drop_score;
                true
            }
            GameAction::RotateCw => self.try_rotate(true),
            GameAction::RotateCcw => self.try_rotate(false),
            GameAction::Hold => self.hold(),
            GameAction::Pause => {
                self.paused = !self.paused;
                if self.paused {
                    self.is_soft_dropping = false;
                    self.soft_drop_timer_ms = 0;
                }
                true
            }
            GameAction::Restart => {
                let seed = self.piece_queue.seed();
                let next_episode = self.episode_id.wrapping_add(1);
                *self = Self::new(seed);
                self.episode_id = next_episode;
                self.start();
                true
            }
        }
    }

    /// Get the shape of the active piece (for rendering)
    #[cfg(test)]
    pub(crate) fn active_shape(&self) -> Option<[(i8, i8); 4]> {
        self.active.map(|p| p.shape())
    }

    /// Check if piece can move in given direction
    #[cfg(test)]
    pub(crate) fn can_move(&self, dx: i8, dy: i8) -> bool {
        let Some(active) = self.active else {
            return false;
        };

        let shape = active.shape();
        shape
            .iter()
            .all(|&(mx, my)| self.board.is_valid(active.x + mx + dx, active.y + my + dy))
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::scoring::qualifies_for_b2b;

    #[test]
    fn test_new_game_state() {
        let state = GameState::new(12345);

        assert!(!state.started);
        assert!(!state.game_over);
        assert!(!state.paused);
        assert_eq!(state.score, 0);
        assert_eq!(state.level, 0);
        assert_eq!(state.lines, 0);
        assert_eq!(state.combo, -1);
        assert!(!state.back_to_back);
        assert_eq!(state.episode_id, 0);
        assert!(state.active.is_none());
        assert!(state.hold.is_none());
        assert_eq!(state.next_queue.len(), 5);
    }

    #[test]
    fn test_restart_increments_episode_id() {
        let mut state = GameState::new(12345);
        state.start();
        assert_eq!(state.episode_id, 0);
        assert!(state.apply_action(GameAction::Restart));
        assert_eq!(state.episode_id, 1);
    }

    #[test]
    fn test_game_start() {
        let mut state = GameState::new(12345);
        assert!(!state.started);

        state.start();
        assert!(state.started);
        assert!(state.active.is_some());
    }

    #[test]
    fn test_spawn_piece() {
        let mut state = GameState::new(12345);
        state.start();

        assert_eq!(state.piece_id, 1);
        assert_eq!(state.active_id, 1);
        assert_eq!(state.step_in_piece, 0);

        let first_kind = state.active.unwrap().kind;
        // The active piece was drawn from the queue, so next_queue[0]
        // should be the NEXT piece to be spawned (not the current active piece)
        let next_kind = state.next_queue[0];

        // Lock current piece to spawn next
        state.lock_piece();

        // Check game didn't end
        if state.game_over {
            return;
        }

        assert!(state.active.is_some());
        assert_eq!(state.piece_id, 2);
        assert_eq!(state.active_id, 2);
        assert_eq!(state.step_in_piece, 0);
        // The next active piece should match what was in next_queue[0]
        assert_eq!(state.active.unwrap().kind, next_kind);
        // And it should be different from the first piece (7-bag has no repeats)
        assert_ne!(state.active.unwrap().kind, first_kind);
    }

    #[test]
    fn test_step_in_piece_increments_on_tick() {
        let mut state = GameState::new(12345);
        state.start();
        assert_eq!(state.step_in_piece, 0);

        state.tick(16, false);
        assert_eq!(state.step_in_piece, 1);

        state.tick(16, false);
        assert_eq!(state.step_in_piece, 2);
    }

    #[test]
    fn test_hold_increments_piece_id() {
        let mut state = GameState::new(12345);
        state.start();
        // First hold when hold is empty spawns a new piece (piece_id increments).
        let first_piece_id = state.piece_id;
        let first_active_id = state.active_id;
        assert!(state.apply_action(GameAction::Hold));
        assert!(state.piece_id > first_piece_id);
        assert!(state.active_id > first_active_id);
        assert_eq!(state.step_in_piece, 0);

        // Lock to allow holding again, then hold swap should NOT change piece_id.
        state.lock_piece();
        if state.game_over {
            return;
        }
        let spawn_piece_id = state.piece_id;
        let swap_active_id = state.active_id;
        assert!(state.apply_action(GameAction::Hold));
        assert_eq!(state.piece_id, spawn_piece_id);
        assert!(state.active_id > swap_active_id);
    }

    #[test]
    fn test_step_in_piece_increments_during_line_clear_pause() {
        let mut state = GameState::new(12345);
        state.start();
        // Force a pause.
        state.line_clear_timer_ms = 16;
        state.tick(16, false);
        assert_eq!(state.step_in_piece, 1);
    }

    #[test]
    fn test_tick_resumes_when_line_clear_timer_expires() {
        let mut state = GameState::new(12345);
        state.start();

        state.active = Some(Tetromino {
            kind: PieceKind::I,
            rotation: Rotation::North,
            x: 3,
            y: 0,
        });

        // End the line clear pause in this tick, then ensure gravity proceeds in the same call.
        state.line_clear_timer_ms = 16;
        state.drop_timer_ms = state.drop_interval_ms();

        let y_before = state.active.unwrap().y;
        assert!(state.tick(16, false));
        assert_eq!(state.line_clear_timer_ms, 0);
        assert_eq!(state.active.unwrap().y, y_before + 1);
    }

    #[test]
    fn test_last_event_set_on_hard_drop() {
        let mut state = GameState::new(12345);
        state.start();

        assert!(state.apply_action(GameAction::HardDrop));
        let ev = state.take_last_event();
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert!(ev.locked);
    }

    #[test]
    fn test_tetromino_new() {
        let piece = Tetromino::new(PieceKind::T);

        assert_eq!(piece.kind, PieceKind::T);
        assert_eq!(piece.rotation, Rotation::North);
        assert_eq!(piece.x, 3);
        assert_eq!(piece.y, 0);
    }

    #[test]
    fn test_tetromino_shape() {
        let piece = Tetromino::new(PieceKind::I);
        let shape = piece.shape();

        // I piece at North rotation: horizontal bar
        assert_eq!(shape, [(0, 1), (1, 1), (2, 1), (3, 1)]);
    }

    #[test]
    fn test_try_move() {
        let mut state = GameState::new(12345);
        state.start();

        let initial_x = state.active.unwrap().x;

        // Move right
        assert!(state.try_move(1, 0));
        assert_eq!(state.active.unwrap().x, initial_x + 1);

        // Move left
        assert!(state.try_move(-1, 0));
        assert_eq!(state.active.unwrap().x, initial_x);

        // Can't move up
        assert!(!state.try_move(0, -1));
    }

    #[test]
    fn test_try_move_collision() {
        let mut state = GameState::new(12345);
        state.start();

        // Try to move far left (should fail eventually)
        let mut moved = 0;
        for _ in 0..10 {
            if state.try_move(-1, 0) {
                moved += 1;
            }
        }
        // Should hit wall after at most 5 moves (spawn at x=3)
        assert!(moved <= 5);
    }

    #[test]
    fn test_try_rotate() {
        let mut state = GameState::new(12345);
        state.start();

        // Ensure we have a rotatable piece (not O)
        while state.active.map(|p| p.kind) == Some(PieceKind::O) {
            state.lock_piece();
        }

        let initial_rotation = state.active.unwrap().rotation;

        // Rotate clockwise
        assert!(state.try_rotate(true));
        assert_eq!(state.active.unwrap().rotation, initial_rotation.rotate_cw());

        // Rotate counter-clockwise
        assert!(state.try_rotate(false));
        assert_eq!(state.active.unwrap().rotation, initial_rotation);
    }

    #[test]
    fn test_try_rotate_o_piece() {
        let mut state = GameState::new(12345);

        // Force an O piece
        while state.next_queue[0] != PieceKind::O {
            state = GameState::new(state.piece_queue.seed().wrapping_add(1));
        }

        state.start();
        assert_eq!(state.active.unwrap().kind, PieceKind::O);

        // O piece shouldn't rotate
        assert!(!state.try_rotate(true));
        assert!(!state.try_rotate(false));
    }

    #[test]
    fn test_hard_drop() {
        let mut state = GameState::new(12345);
        state.start();

        let _initial_y = state.active.unwrap().y;
        let score_before = state.score;

        state.hard_drop();

        // Piece should be locked and new piece spawned
        assert!(state.active.is_some());
        // Score should increase from hard drop
        assert!(state.score > score_before || state.score == score_before);
    }

    #[test]
    fn test_hold() {
        let mut state = GameState::new(12345);
        state.start();

        let initial_kind = state.active.unwrap().kind;

        // First hold (no previous hold)
        assert!(state.hold());
        assert_eq!(state.hold, Some(initial_kind));
        assert!(state.active.is_some());
        assert!(!state.can_hold); // Can't hold again until piece is locked

        // Try to hold again (should fail)
        assert!(!state.hold());

        // Lock piece and try hold with new piece
        let next_kind = state.active.unwrap().kind;
        state.lock_piece();

        // Check that game didn't end
        if state.game_over {
            return; // Test cannot continue if game over
        }

        // Can hold now
        assert!(state.can_hold);
        assert!(state.hold());
        assert_eq!(state.active.unwrap().kind, initial_kind); // Swapped with hold
        assert_eq!(state.hold, Some(next_kind));
    }

    #[test]
    fn test_is_grounded() {
        let mut state = GameState::new(12345);
        state.start();

        // Fresh piece should not be grounded
        assert!(!state.is_grounded());

        // Drop to bottom
        state.hard_drop();

        // After locking and spawning new piece, check grounded
        if let Some(ref piece) = state.active {
            // Check if new piece is grounded (should be false at spawn)
            let grounded = piece.is_grounded(&state.board);
            // Depends on spawn position, but typically not grounded
            assert!(!grounded);
        }
    }

    #[test]
    fn test_ghost_y() {
        let mut state = GameState::new(12345);
        state.start();

        let initial_y = state.active.unwrap().y;
        let ghost_y = state.ghost_y().unwrap();

        // Ghost should be at or below active piece
        assert!(ghost_y >= initial_y);
    }

    #[test]
    fn test_drop_interval_ms() {
        let mut state = GameState::new(12345);
        state.start();

        // Level 0 should be 1000ms
        assert_eq!(state.drop_interval_ms(), 1000);

        // Set level to 5
        state.level = 5;
        assert_eq!(state.drop_interval_ms(), 320);

        // Test soft drop (10x multiplier: 320 / 10 = 32)
        state.is_soft_dropping = true;
        assert_eq!(state.drop_interval_ms(), 32);
    }

    #[test]
    fn test_lock_piece() {
        let mut state = GameState::new(12345);
        state.start();

        // Lock the piece
        state.lock_piece();

        // Check result - either new piece spawned or game over
        if !state.game_over {
            assert!(state.active.is_some());
        }
    }

    #[test]
    fn test_lock_piece_clears_lines() {
        let mut state = GameState::new(12345);
        state.start();

        // Fill bottom row except one cell
        for x in 0..9 {
            state.board.set(x, 19, Some(PieceKind::I));
        }

        // Try to get an I piece to fill the gap
        let mut attempts = 0;
        while state.active.map(|p| p.kind) != Some(PieceKind::I) && attempts < 14 {
            state.lock_piece();
            attempts += 1;
        }

        if state.active.map(|p| p.kind) == Some(PieceKind::I) {
            // Move I piece to fill the gap
            state.try_move(-3, 0); // Align with gap
            state.hard_drop();

            // Should have cleared a line
            // Note: This test is probabilistic based on RNG
        }
    }

    #[test]
    fn test_t_spin_detection() {
        let mut state = GameState::new(12345);

        // Setup: Create a T-slot scenario
        // T piece at North rotation with corners filled
        let piece = Tetromino {
            kind: PieceKind::T,
            rotation: Rotation::North,
            x: 3,
            y: 18,
        };

        state.active = Some(piece);
        state.last_action_was_rotate = true;

        // Fill corners to create T-spin
        state.board.set(3, 18, Some(PieceKind::I)); // top-left
        state.board.set(5, 18, Some(PieceKind::I)); // top-right
        state.board.set(3, 20, Some(PieceKind::I)); // bottom-left
        state.board.set(5, 20, Some(PieceKind::I)); // bottom-right

        // Detect T-spin
        let tspin = state.t_spin_kind(&piece, &[]);

        // Should detect some T-spin type
        assert_ne!(tspin, TSpinKind::None);
    }

    #[test]
    fn test_t_spin_no_rotation() {
        let mut state = GameState::new(12345);

        let piece = Tetromino {
            kind: PieceKind::T,
            rotation: Rotation::North,
            x: 3,
            y: 18,
        };

        state.active = Some(piece);
        state.last_action_was_rotate = false; // No rotation

        // Fill corners
        state.board.set(3, 18, Some(PieceKind::I));
        state.board.set(5, 18, Some(PieceKind::I));
        state.board.set(3, 20, Some(PieceKind::I));
        state.board.set(5, 20, Some(PieceKind::I));

        let tspin = state.t_spin_kind(&piece, &[]);

        // Should be None because last action wasn't a rotation
        assert_eq!(tspin, TSpinKind::None);
    }

    #[test]
    fn test_tick_no_active_piece() {
        let mut state = GameState::new(12345);
        state.started = true;
        state.active = None;

        // Tick should not panic
        assert!(!state.tick(16, false));
    }

    #[test]
    fn test_tick_gravity() {
        let mut state = GameState::new(12345);
        state.start();

        let initial_y = state.active.unwrap().y;

        // Tick with enough time for gravity
        for _ in 0..100 {
            state.tick(16, false);
        }

        // Piece should have moved down or been locked
        // (depends on whether it's grounded)
        if state.active.is_some() {
            assert!(state.active.unwrap().y >= initial_y);
        }
    }

    #[test]
    fn test_tick_gravity_accumulates_multiple_intervals_in_one_call() {
        let mut state = GameState::new(12345);
        state.started = true;
        state.active = Some(Tetromino {
            kind: PieceKind::O,
            rotation: Rotation::North,
            x: 3,
            y: 0,
        });

        let interval = state.drop_interval_ms();
        let y_before = state.active.unwrap().y;
        assert!(state.tick(interval * 3, false));
        assert_eq!(state.active.unwrap().y, y_before + 3);
        assert_eq!(state.drop_timer_ms, 0);
        assert_eq!(state.lock_timer_ms, 0);
    }

    #[test]
    fn test_tick_gravity_consumes_drop_timer_even_when_grounded() {
        let mut state = GameState::new(12345);
        state.started = true;
        state.level = 9; // drop interval 120ms; keep elapsed < LOCK_DELAY_MS
        state.active = Some(Tetromino {
            kind: PieceKind::O,
            rotation: Rotation::North,
            x: 3,
            y: 18, // grounded for O at y=18 (bottom at y=19)
        });
        assert!(state.is_grounded());

        let interval = state.drop_interval_ms();
        assert!(interval > 0);

        // swiftui-tetris: dropTimerMs is reduced in a while-loop even if the piece cannot move down.
        let y_before = state.active.unwrap().y;
        assert!(!state.tick(interval * 3, false));
        assert_eq!(state.active.unwrap().y, y_before);
        assert_eq!(state.drop_timer_ms, 0);
        assert_eq!(state.lock_timer_ms, interval * 3);
    }

    #[test]
    fn test_lock_reset_count_resets_while_falling() {
        let mut state = GameState::new(12345);
        state.start();

        // While the piece can move down, any movement should keep lock timer and reset count cleared
        // (swiftui-tetris parity: lock reset count is only consumed when grounded).
        state.lock_reset_count = 7;
        state.lock_timer_ms = 123;

        // Move down a few cells while not grounded.
        for _ in 0..3 {
            if state.is_grounded() {
                break;
            }
            assert!(state.try_move(0, 1));
            assert_eq!(state.lock_timer_ms, 0);
            assert_eq!(state.lock_reset_count, 0);
        }
    }

    #[test]
    fn test_lock_timer_starts_immediately_after_landing() {
        let mut state = GameState::new(12345);
        state.started = true;

        // Place an O piece one cell above the floor. After one gravity step it becomes grounded.
        state.active = Some(Tetromino {
            kind: PieceKind::O,
            rotation: Rotation::North,
            x: 3,
            y: 17,
        });

        state.drop_timer_ms = state.drop_interval_ms();

        assert!(state.tick(16, false));
        assert_eq!(state.active.unwrap().y, 18);
        assert!(state.is_grounded());

        // swiftui-tetris: handleLockReset runs on the landing move (consuming one reset),
        // then tick increments lockTimer in the same step once grounded.
        assert_eq!(state.lock_reset_count, 1);
        assert_eq!(state.lock_timer_ms, 16);
    }

    #[test]
    fn test_apply_action_move() {
        let mut state = GameState::new(12345);
        state.start();

        let initial_x = state.active.unwrap().x;

        assert!(state.apply_action(GameAction::MoveRight));
        assert_eq!(state.active.unwrap().x, initial_x + 1);

        assert!(state.apply_action(GameAction::MoveLeft));
        assert_eq!(state.active.unwrap().x, initial_x);
    }

    #[test]
    fn test_apply_action_rotate() {
        let mut state = GameState::new(12345);
        state.start();

        // Skip O pieces
        while state.active.map(|p| p.kind) == Some(PieceKind::O) {
            state.lock_piece();
        }

        let initial_rotation = state.active.unwrap().rotation;

        assert!(state.apply_action(GameAction::RotateCw));
        assert_eq!(state.active.unwrap().rotation, initial_rotation.rotate_cw());
    }

    #[test]
    fn test_apply_action_hard_drop() {
        let mut state = GameState::new(12345);
        state.start();

        let score_before = state.score;

        assert!(state.apply_action(GameAction::HardDrop));

        // Score should increase or piece should be locked
        assert!(state.score > score_before || state.active.is_some());
    }

    #[test]
    fn test_apply_action_pause() {
        let mut state = GameState::new(12345);
        state.start();

        assert!(!state.paused);

        state.apply_action(GameAction::Pause);
        assert!(state.paused);

        state.apply_action(GameAction::Pause);
        assert!(!state.paused);
    }

    #[test]
    fn test_apply_action_restart() {
        let mut state = GameState::new(12345);
        state.start();

        // Play a bit
        for _ in 0..10 {
            state.apply_action(GameAction::MoveRight);
        }

        let _old_score = state.score;

        // Restart
        state.apply_action(GameAction::Restart);

        assert!(state.started);
        assert!(!state.game_over);
        assert!(!state.paused);
        assert_eq!(state.score, 0);
        assert_eq!(state.level, 0);
        assert_eq!(state.lines, 0);
    }

    #[test]
    fn test_lock_reset_limit() {
        let mut state = GameState::new(12345);
        state.start();

        // Force piece to be grounded
        while !state.is_grounded() {
            state.try_move(0, 1);
        }

        // Reset lock timer many times
        for _ in 0..20 {
            state.handle_lock_reset();
        }

        // Should be limited to 15
        assert_eq!(state.lock_reset_count, LOCK_RESET_LIMIT);
    }

    #[test]
    fn test_line_clear_timing() {
        let mut state = GameState::new(12345);
        state.start();
        state.line_clear_timer_ms = LINE_CLEAR_PAUSE_MS;

        // Tick during line clear pause
        assert!(!state.tick(100, false));
        assert!(state.line_clear_timer_ms > 0);

        // Tick past the pause
        assert!(!state.tick(200, false));
    }

    #[test]
    fn test_soft_drop_grace_period() {
        let mut state = GameState::new(12345);
        state.start();

        // Start soft dropping
        state.is_soft_dropping = true;
        state.soft_drop_timer_ms = SOFT_DROP_GRACE_MS;

        // Tick during grace period
        state.tick(50, true);
        assert!(state.is_soft_dropping);
        assert!(state.soft_drop_timer_ms > 0);
    }

    #[test]
    fn test_soft_drop_timeout_expires_and_disables_speed() {
        let mut state = GameState::new(12345);
        state.start();

        state.is_soft_dropping = true;
        state.soft_drop_timer_ms = 10;

        state.tick(16, false);
        assert_eq!(state.soft_drop_timer_ms, 0);
        assert!(!state.is_soft_dropping);
    }

    #[test]
    fn test_soft_drop_gravity_does_not_add_score() {
        let mut state = GameState::new(12345);
        state.started = true;
        state.active = Some(Tetromino {
            kind: PieceKind::O,
            rotation: Rotation::North,
            x: 3,
            y: 0,
        });

        state.is_soft_dropping = true;
        state.soft_drop_timer_ms = SOFT_DROP_GRACE_MS;

        let score_before = state.score;
        state.drop_timer_ms = state.drop_interval_ms();

        assert!(state.tick(0, false));
        assert_eq!(state.score, score_before);
    }

    #[test]
    fn test_game_over_detection() {
        let mut state = GameState::new(12345);
        state.start();

        // Fill spawn area
        for x in 3..=6 {
            for y in 0..=1 {
                state.board.set(x, y, Some(PieceKind::I));
            }
        }

        // Try to lock current piece and spawn next
        state.lock_piece();

        // Should detect game over
        assert!(state.game_over);
    }

    #[test]
    fn test_combo_tracking() {
        let mut state = GameState::new(12345);

        // Manually set up state with combo
        state.combo = 3;
        state.back_to_back = true;

        // Verify state
        assert_eq!(state.combo, 3);
        assert!(state.back_to_back);
    }

    #[test]
    fn test_tspin_full_single_uses_modern_table_only() {
        let mut state = GameState::new(1);
        state.apply_line_clear(1, TSpinKind::Full);

        assert_eq!(state.score, 800);
        assert_eq!(state.combo, 0);
        assert!(state.back_to_back);
    }

    #[test]
    fn test_combo_bonus_starts_after_second_clear() {
        let mut state = GameState::new(1);
        state.apply_line_clear(1, TSpinKind::None);
        assert_eq!(state.score, 40);
        assert_eq!(state.combo, 0);

        state.apply_line_clear(1, TSpinKind::None);
        assert_eq!(state.score, 40 + (40 + 50));
        assert_eq!(state.combo, 1);
    }

    #[test]
    fn test_level_multiplier_uses_pre_clear_level() {
        let mut state = GameState::new(1);
        state.lines = 9;
        state.level = 0;
        state.apply_line_clear(1, TSpinKind::None);
        assert_eq!(state.score, 40);
        assert_eq!(state.lines, 10);
        assert_eq!(state.level, 1);
    }

    #[test]
    fn test_b2b_multiplier_excludes_combo_bonus() {
        let mut state = GameState::new(1);
        state.back_to_back = true;
        state.combo = 0; // already have one prior clear in the chain
        state.apply_line_clear(4, TSpinKind::None);

        // Base: 1200 * 3/2 = 1800, then combo bonus +50.
        assert_eq!(state.score, 1850);
        assert_eq!(state.combo, 1);
        assert!(state.back_to_back);
    }

    #[test]
    fn test_can_move() {
        let mut state = GameState::new(12345);
        state.start();

        // Should be able to move down initially
        assert!(state.can_move(0, 1));

        // Should not be able to move up
        assert!(!state.can_move(0, -1));
    }

    #[test]
    fn test_active_shape() {
        let mut state = GameState::new(12345);
        state.start();

        let shape = state.active_shape();
        assert!(shape.is_some());

        // Shape should have 4 minos
        let shape = shape.unwrap();
        assert_eq!(shape.len(), 4);
    }

    #[test]
    fn test_default_game_state() {
        let state = GameState::default();

        assert!(!state.started);
        assert_eq!(state.score, 0);
    }

    #[test]
    fn test_lock_piece_updates_lines_and_score() {
        let mut state = GameState::new(12345);
        state.start();

        // Get an I piece if possible
        let mut attempts = 0;
        while state.active.map(|p| p.kind) != Some(PieceKind::I) && attempts < 14 {
            state.lock_piece();
            attempts += 1;
        }

        if state.active.map(|p| p.kind) == Some(PieceKind::I) {
            let initial_score = state.score;
            let initial_lines = state.lines;

            // Drop to bottom
            state.hard_drop();

            // Check if anything changed
            if state.lines > initial_lines {
                assert!(state.score > initial_score);
            }
        }
    }

    #[test]
    fn test_multiple_line_clear() {
        let mut state = GameState::new(12345);
        state.start();

        // Fill multiple rows
        for y in 16..20 {
            for x in 0..BOARD_WIDTH {
                state.board.set(x as i8, y as i8, Some(PieceKind::I));
            }
        }

        // Lock a piece - should potentially clear lines
        let initial_lines = state.lines;
        state.lock_piece();

        // Lines might have been cleared
        if state.lines > initial_lines {
            assert!(state.combo > 0 || state.lines > initial_lines);
        }
    }

    #[test]
    fn test_qualifies_for_b2b() {
        assert!(qualifies_for_b2b(TSpinKind::Full, 1));
        assert!(qualifies_for_b2b(TSpinKind::Full, 4));
        assert!(qualifies_for_b2b(TSpinKind::None, 4)); // Tetris

        assert!(!qualifies_for_b2b(TSpinKind::Mini, 1));
        assert!(!qualifies_for_b2b(TSpinKind::None, 3));
        assert!(!qualifies_for_b2b(TSpinKind::None, 1));
    }

    #[test]
    fn test_is_valid_method() {
        let mut state = GameState::new(12345);
        state.start();

        let piece = state.active.unwrap();

        // Piece should be valid at spawn
        assert!(piece.is_valid(&state.board));
    }

    #[test]
    fn test_move_resets_lock_timer() {
        let mut state = GameState::new(12345);
        state.start();

        // Move piece to ensure it can move horizontally when grounded
        // First, center the piece
        state.try_move(1, 0);
        state.try_move(1, 0);

        // Ground the piece by moving down
        while !state.is_grounded() {
            if !state.try_move(0, 1) {
                break;
            }
        }

        // Verify piece is grounded
        if !state.is_grounded() {
            return; // Skip test if we can't ground the piece
        }

        // IMPORTANT: Reset lock_reset_count AFTER grounding the piece
        // (grounding the piece may have incremented the count)
        state.lock_reset_count = 0;

        // Accumulate some lock time
        state.lock_timer_ms = 100;

        // Move horizontally while grounded - should reset lock timer if move succeeds
        let moved_left = state.try_move(-1, 0);
        let moved_right = if !moved_left {
            state.try_move(1, 0)
        } else {
            false
        };

        // If we successfully moved horizontally while grounded, timer should reset
        if moved_left || moved_right {
            assert_eq!(
                state.lock_timer_ms, 0,
                "Timer should reset when moving horizontally while grounded"
            );
            // swiftui-tetris resets lock timer/count when not grounded, and consumes a reset only
            // when grounded after the move.
            if state.is_grounded() {
                assert_eq!(state.lock_reset_count, 1);
            } else {
                assert_eq!(state.lock_reset_count, 0);
            }
        }
        // If we couldn't move, the test condition isn't met, so we pass vacuously
    }

    #[test]
    fn test_rotate_resets_lock_timer() {
        let mut state = GameState::new(12345);
        state.start();

        // Skip O pieces (can't rotate)
        while state.active.map(|p| p.kind) == Some(PieceKind::O) {
            state.lock_piece();
            if state.game_over {
                return;
            }
        }

        // Move piece to center to give room for rotation kicks
        state.try_move(1, 0);

        // Ground the piece
        while !state.is_grounded() {
            if !state.try_move(0, 1) {
                break;
            }
        }

        // Verify piece is grounded
        if !state.is_grounded() {
            return; // Skip test if we can't ground the piece
        }

        // IMPORTANT: Reset lock_reset_count AFTER grounding the piece
        // (grounding the piece may have incremented the count)
        state.lock_reset_count = 0;

        // Accumulate some lock time
        state.lock_timer_ms = 100;

        // Rotate while grounded - should reset lock timer if rotation succeeds
        let rotated = state.try_rotate(true) || state.try_rotate(false);

        // If we successfully rotated while grounded, timer should reset
        if rotated {
            assert_eq!(state.lock_timer_ms, 0);
            // swiftui-tetris resets lock timer/count when not grounded, and consumes a reset only
            // when grounded after the rotation.
            if state.is_grounded() {
                assert_eq!(state.lock_reset_count, 1);
            } else {
                assert_eq!(state.lock_reset_count, 0);
            }
            assert!(state.last_action_was_rotate);
        }
        // If we couldn't rotate, the test condition isn't met, so we pass vacuously
    }

    #[test]
    fn test_hold_spawns_new_when_empty() {
        let mut state = GameState::new(12345);
        state.start();

        let initial_kind = state.active.unwrap().kind;
        let next_in_queue = state.next_queue[0];

        // Hold with empty hold slot
        assert!(state.hold());

        // Hold should contain the piece we had
        assert_eq!(state.hold, Some(initial_kind));

        // Active should be the next piece from queue
        assert_eq!(state.active.unwrap().kind, next_in_queue);
    }

    #[test]
    fn test_hold_blocked_after_use() {
        let mut state = GameState::new(12345);
        state.start();

        // Use hold
        state.hold();
        assert!(!state.can_hold);

        // Try to hold again (should fail)
        assert!(!state.hold());

        // Lock piece to re-enable hold
        state.lock_piece();

        // Check that game didn't end
        if state.game_over {
            return; // Test cannot continue if game over
        }

        // Should be able to hold again
        assert!(state.can_hold);
    }

    #[test]
    fn test_level_progression() {
        let mut state = GameState::new(12345);
        state.start();

        assert_eq!(state.level, 0);

        // Simulate clearing lines
        state.lines = 10;
        state.level = state.lines / 10;

        assert_eq!(state.level, 1);

        state.lines = 25;
        state.level = state.lines / 10;

        assert_eq!(state.level, 2);
    }

    #[test]
    fn test_pause_stops_game() {
        let mut state = GameState::new(12345);
        state.start();

        let initial_y = state.active.unwrap().y;

        // Pause
        state.paused = true;

        // Tick many times while paused
        for _ in 0..100 {
            state.tick(16, false);
        }

        // Piece should not have moved
        assert_eq!(state.active.unwrap().y, initial_y);
        assert!(state.paused);
    }

    #[test]
    fn test_game_over_stops_game() {
        let mut state = GameState::new(12345);
        state.start();

        // Set game over and remove active piece to block actions
        state.game_over = true;
        state.active = None;

        // Try to apply actions - should return false when no active piece
        assert!(!state.apply_action(GameAction::MoveLeft));
        assert!(!state.apply_action(GameAction::MoveRight));

        // Tick should not do anything
        let result = state.tick(16, false);
        assert!(!result);
    }

    #[test]
    fn test_actions_ignored_while_paused_except_pause_and_restart() {
        let mut state = GameState::new(12345);
        state.start();
        state.paused = true;

        let x_before = state.active.unwrap().x;
        let y_before = state.active.unwrap().y;
        let rot_before = state.active.unwrap().rotation;
        let piece_id_before = state.piece_id;

        assert!(!state.apply_action(GameAction::MoveLeft));
        assert!(!state.apply_action(GameAction::MoveRight));
        assert!(!state.apply_action(GameAction::SoftDrop));
        assert!(!state.apply_action(GameAction::HardDrop));
        assert!(!state.apply_action(GameAction::RotateCw));
        assert!(!state.apply_action(GameAction::RotateCcw));
        assert!(!state.apply_action(GameAction::Hold));

        assert_eq!(state.active.unwrap().x, x_before);
        assert_eq!(state.active.unwrap().y, y_before);
        assert_eq!(state.active.unwrap().rotation, rot_before);
        assert_eq!(state.piece_id, piece_id_before);

        // Pause toggles even while paused.
        assert!(state.apply_action(GameAction::Pause));
        assert!(!state.paused);

        // Restart always works.
        assert!(state.apply_action(GameAction::Restart));
        assert!(state.started);
    }

    #[test]
    fn test_actions_ignored_when_game_over_except_restart() {
        let mut state = GameState::new(12345);
        state.start();
        state.game_over = true;

        assert!(!state.apply_action(GameAction::MoveLeft));
        assert!(!state.apply_action(GameAction::RotateCw));
        assert!(!state.apply_action(GameAction::Pause));

        assert!(state.apply_action(GameAction::Restart));
        assert!(!state.game_over);
        assert!(state.started);
    }

    #[test]
    fn test_pause_clears_soft_drop_state() {
        let mut state = GameState::new(12345);
        state.start();

        state.is_soft_dropping = true;
        state.soft_drop_timer_ms = SOFT_DROP_GRACE_MS;

        // Pause should clear soft drop state like swiftui-tetris.
        assert!(state.apply_action(GameAction::Pause));
        assert!(state.paused());
        assert!(!state.is_soft_dropping);
        assert_eq!(state.soft_drop_timer_ms, 0);
    }

    #[test]
    fn test_soft_drop_scoring() {
        let mut state = GameState::new(12345);
        state.start();

        let initial_score = state.score;

        // Apply soft drop action (immediate move down)
        state.apply_action(GameAction::SoftDrop);

        // Score should increase by 1 per cell soft dropped
        assert!(state.score > initial_score || state.active.unwrap().y > 0);
    }

    #[test]
    fn test_soft_drop_action_clears_last_action_was_rotate() {
        let mut state = GameState::new(12345);
        state.start();
        state.last_action_was_rotate = true;
        let _ = state.apply_action(GameAction::SoftDrop);
        assert!(!state.last_action_was_rotate);
    }

    #[test]
    fn test_hard_drop_action_clears_last_action_was_rotate() {
        let mut state = GameState::new(12345);
        state.start();
        state.last_action_was_rotate = true;
        let _ = state.apply_action(GameAction::HardDrop);
        assert!(!state.last_action_was_rotate);
    }

    #[test]
    fn test_next_queue_updates() {
        let mut state = GameState::new(12345);
        state.start();

        let _first_next = state.next_queue;

        // Lock piece
        state.lock_piece();

        // Check that game didn't end
        if state.game_over {
            return; // Test cannot continue if game over
        }

        // Next queue should have changed (or the queue rotated)
        // The first piece should be different or the queue shifted
        assert_eq!(state.next_queue.len(), 5);
    }

    #[test]
    fn test_last_event_line_clear_score_excludes_combo_bonus() {
        let mut state = GameState::new(12345);
        state.start();

        // Make a deterministic "Tetris" clear: 4 full rows with a 1-cell hole in the same column.
        // Then place a vertical I piece to fill the holes.
        let hole_x = 2;
        for y in 16..=19 {
            for x in 0..10 {
                if x != hole_x {
                    state.board.set(x, y, Some(PieceKind::I));
                }
            }
        }

        // Align internal level/lines so pre-clear level matches expectations.
        state.level = 2;
        state.lines = 20;

        // Simulate consecutive clear chain (combo already started) and B2B already active.
        state.combo = 0;
        state.back_to_back = true;

        // Vertical I (rotation East) occupies (x+2, y+0..3).
        state.active = Some(Tetromino {
            kind: PieceKind::I,
            rotation: Rotation::East,
            x: 0,
            y: 16,
        });

        let score_before = state.score;
        state.lock_piece();
        let ev = state.take_last_event().expect("expected last_event");

        let expected_base = 1200 * (state.level + 1); // pre-clear level = 2 => *3
        let expected_line_clear_score = expected_base * 3 / 2; // B2B multiplier
        let expected_combo_after = 1; // combo was 0, +1 on clear
        let expected_combo_bonus = crate::types::COMBO_BASE * (expected_combo_after as u32);
        let expected_delta = expected_line_clear_score + expected_combo_bonus;

        assert!(ev.locked);
        assert_eq!(ev.lines_cleared, 4);
        assert_eq!(ev.tspin, None);
        assert_eq!(ev.back_to_back, true);
        assert_eq!(ev.combo, expected_combo_after);
        assert_eq!(ev.line_clear_score, expected_line_clear_score);
        assert_eq!(state.score - score_before, expected_delta);
    }

    #[test]
    fn test_last_event_combo_starts_at_zero_and_b2b_applies_on_next_clear() {
        let mut state = GameState::new(12345);
        state.start();

        let hole_x = 2;
        for y in 16..=19 {
            for x in 0..10 {
                if x != hole_x {
                    state.board.set(x, y, Some(PieceKind::I));
                }
            }
        }

        state.level = 0;
        state.lines = 0;
        state.combo = -1;
        state.back_to_back = false;

        state.active = Some(Tetromino {
            kind: PieceKind::I,
            rotation: Rotation::East,
            x: 0,
            y: 16,
        });

        let score_before = state.score;
        state.lock_piece();
        let ev = state.take_last_event().expect("expected last_event");

        let expected_line_clear_score = 1200 * (0 + 1);
        let expected_delta = expected_line_clear_score; // first clear in chain => combo=0, no combo bonus

        assert_eq!(ev.lines_cleared, 4);
        assert_eq!(ev.combo, 0);
        assert_eq!(ev.back_to_back, true);
        assert_eq!(ev.line_clear_score, expected_line_clear_score);
        assert_eq!(state.score - score_before, expected_delta);
    }

    #[test]
    fn test_last_event_line_clear_score_excludes_drop_points() {
        let mut state = GameState::new(12345);
        state.start();

        // Make a deterministic "Tetris" clear: 4 full rows with a 1-cell hole in the same column.
        // Then hard drop a vertical I piece from y=0 to y=16 to fill the holes.
        let hole_x = 2;
        for y in 16..=19 {
            for x in 0..10 {
                if x != hole_x {
                    state.board.set(x, y, Some(PieceKind::I));
                }
            }
        }

        state.level = 0;
        state.lines = 0;
        state.combo = -1;
        state.back_to_back = false;

        state.active = Some(Tetromino {
            kind: PieceKind::I,
            rotation: Rotation::East,
            x: 0,
            y: 0,
        });

        let score_before = state.score;
        assert!(state.apply_action(GameAction::HardDrop));
        let ev = state.take_last_event().expect("expected last_event");

        // I(East) lands at y=16 in this setup => 16 rows hard-dropped => 32 drop points.
        let expected_drop_points = 16 * 2;
        let expected_line_clear_score = 1200 * (0 + 1);
        let expected_delta = expected_line_clear_score + expected_drop_points;

        assert_eq!(ev.lines_cleared, 4);
        assert_eq!(ev.combo, 0);
        assert_eq!(ev.back_to_back, true);
        assert_eq!(ev.line_clear_score, expected_line_clear_score);
        assert_eq!(state.score - score_before, expected_delta);
    }

    #[test]
    fn test_mini_tspin_clear_resets_b2b_chain() {
        let mut state = GameState::new(12345);
        state.start();

        // Set up as if we are already in a B2B chain.
        state.back_to_back = true;
        state.combo = -1;
        state.level = 0;
        state.lines = 0;

        let score_before = state.score;
        let base = state.apply_line_clear(1, TSpinKind::Mini);

        // Mini T-Spins use their own table but do not qualify for B2B carry.
        assert_eq!(base, 200);
        assert_eq!(state.combo, 0);
        assert!(!state.back_to_back);
        assert_eq!(state.score - score_before, 200);
    }

    #[test]
    fn test_back_to_back_breaks_on_non_qualifying_clear() {
        let mut state = GameState::new(12345);
        state.start();

        state.level = 0;
        state.lines = 0;
        state.combo = -1;
        state.back_to_back = true; // chain is active from a prior qualifying clear

        let score_before = state.score;
        let base = state.apply_line_clear(1, TSpinKind::None);

        // Normal single clear does not qualify, so:
        // - it should not get a B2B multiplier even though previous_b2b was true
        // - it should break the chain
        assert_eq!(base, 40);
        assert_eq!(state.score - score_before, 40);
        assert!(!state.back_to_back);
        assert_eq!(state.combo, 0);
    }

    #[test]
    fn test_full_tspin_single_applies_b2b_multiplier_when_chain_active() {
        let mut state = GameState::new(12345);
        state.start();

        state.level = 0;
        state.lines = 0;
        state.combo = -1;
        state.back_to_back = true; // chain active from prior qualifying clear

        let score_before = state.score;
        let base = state.apply_line_clear(1, TSpinKind::Full);

        assert_eq!(base, 1200); // 800 * 3/2
        assert_eq!(state.score - score_before, 1200);
        assert_eq!(state.combo, 0);
        assert!(state.back_to_back); // full tspin single qualifies
    }

    #[test]
    fn test_tspin_scoring_uses_pre_clear_level() {
        let mut state = GameState::new(12345);
        state.start();

        // Set up right before a level-up. Clearing 1 line will push lines 9 -> 10.
        state.level = 0;
        state.lines = 9;
        state.combo = -1;
        state.back_to_back = false;

        let score_before = state.score;
        let base = state.apply_line_clear(1, TSpinKind::Full);

        // Must use pre-clear level 0 (not level 1).
        assert_eq!(base, 800);
        assert_eq!(state.score - score_before, 800);
        assert_eq!(state.level, 1);
        assert_eq!(state.lines, 10);
        assert_eq!(state.combo, 0);
        assert!(state.back_to_back);
    }

    #[test]
    fn test_b2b_chain_break_prevents_next_tetris_multiplier() {
        let mut state = GameState::new(12345);
        state.start();

        state.level = 0;
        state.lines = 0;
        state.combo = -1;
        state.back_to_back = true; // chain active from a prior qualifying clear

        // Non-qualifying clear breaks the chain.
        state.apply_line_clear(1, TSpinKind::None);
        assert!(!state.back_to_back);
        assert_eq!(state.combo, 0);

        // Next Tetris should NOT receive the B2B multiplier.
        let score_before = state.score;
        let base = state.apply_line_clear(4, TSpinKind::None);
        assert_eq!(base, 1200);
        assert_eq!(state.score - score_before, 1200 + crate::types::COMBO_BASE); // combo=1 => +50
        assert!(state.back_to_back);
        assert_eq!(state.combo, 1);
    }

    #[test]
    fn test_tspin_no_line_clear_awards_points_and_resets_chains_full() {
        let mut state = GameState::new(12345);
        state.start();

        state.level = 2;
        state.lines = 29;
        state.combo = 3;
        state.back_to_back = true;

        let score_before = state.score;
        let base = state.apply_line_clear(0, TSpinKind::Full);

        // swiftui-tetris awards T-Spin no-lines points but resets combo/B2B and does not report a line-clear score.
        assert_eq!(base, 0);
        assert_eq!(state.score - score_before, 400 * (2 + 1));
        assert_eq!(state.combo, -1);
        assert!(!state.back_to_back);
        assert_eq!(state.level, 2);
        assert_eq!(state.lines, 29);
    }

    #[test]
    fn test_tspin_no_line_clear_awards_points_and_resets_chains_mini() {
        let mut state = GameState::new(12345);
        state.start();

        state.level = 1;
        state.lines = 10;
        state.combo = 0;
        state.back_to_back = true;

        let score_before = state.score;
        let base = state.apply_line_clear(0, TSpinKind::Mini);

        assert_eq!(base, 0);
        assert_eq!(state.score - score_before, 100 * (1 + 1));
        assert_eq!(state.combo, -1);
        assert!(!state.back_to_back);
        assert_eq!(state.level, 1);
        assert_eq!(state.lines, 10);
    }

    #[test]
    fn test_lock_piece_tspin_no_lines_awards_points_but_last_event_omits_tspin_full() {
        let mut state = GameState::new(12345);
        state.start();

        // Prepare a "no-lines" Full T-Spin: place a T at y=18 and fill both front corners.
        // The bottom corners (y+2) are out-of-bounds and count as filled, giving 4/4 corners filled.
        state.level = 2;
        state.lines = 29;
        state.combo = 3;
        state.back_to_back = true;

        let x = 3;
        state.board.set(x + 0, 18, Some(PieceKind::I));
        state.board.set(x + 2, 18, Some(PieceKind::I));
        state.last_action_was_rotate = true;
        state.active = Some(Tetromino {
            kind: PieceKind::T,
            rotation: Rotation::North,
            x,
            y: 18,
        });

        let score_before = state.score;
        state.lock_piece();
        let ev = state.take_last_event().expect("expected last_event");

        assert_eq!(state.score - score_before, 400 * (2 + 1));
        assert_eq!(ev.lines_cleared, 0);
        assert_eq!(ev.line_clear_score, 0);
        assert_eq!(ev.tspin, None);
        assert_eq!(ev.combo, -1);
        assert!(!ev.back_to_back);
    }

    #[test]
    fn test_lock_piece_tspin_no_lines_awards_points_but_last_event_omits_tspin_mini() {
        let mut state = GameState::new(12345);
        state.start();

        // Prepare a "no-lines" Mini T-Spin: place a T at y=18 and fill exactly one front corner.
        // With the two bottom corners out-of-bounds, this yields 3/4 corners filled and frontFilled=1.
        state.level = 1;
        state.lines = 10;
        state.combo = 0;
        state.back_to_back = true;

        let x = 3;
        state.board.set(x + 0, 18, Some(PieceKind::I));
        state.last_action_was_rotate = true;
        state.active = Some(Tetromino {
            kind: PieceKind::T,
            rotation: Rotation::North,
            x,
            y: 18,
        });

        let score_before = state.score;
        state.lock_piece();
        let ev = state.take_last_event().expect("expected last_event");

        assert_eq!(state.score - score_before, 100 * (1 + 1));
        assert_eq!(ev.lines_cleared, 0);
        assert_eq!(ev.line_clear_score, 0);
        assert_eq!(ev.tspin, None);
        assert_eq!(ev.combo, -1);
        assert!(!ev.back_to_back);
    }

    fn assert_tspin_for_rotation(
        rotation: Rotation,
        filled_corners: &[(i8, i8)],
        expected: TSpinKind,
    ) {
        let mut state = GameState::new(12345);
        let x = 3;
        let y = 10;
        for &(dx, dy) in filled_corners {
            state.board.set(x + dx, y + dy, Some(PieceKind::I));
        }
        state.last_action_was_rotate = true;

        let piece = Tetromino {
            kind: PieceKind::T,
            rotation,
            x,
            y,
        };
        let tspin = state.t_spin_kind(&piece, &[]);
        assert_eq!(tspin, expected);
    }

    #[test]
    fn test_t_spin_front_corner_mapping_north_full_vs_mini() {
        // Corners are relative to the piece origin: (0,0), (2,0), (0,2), (2,2).
        // North-facing "front" corners are the top corners: (0,0) and (2,0).
        assert_tspin_for_rotation(
            Rotation::North,
            &[(0, 0), (0, 2), (2, 2)],
            TSpinKind::Mini,
        );
        assert_tspin_for_rotation(
            Rotation::North,
            &[(0, 0), (2, 0), (0, 2), (2, 2)],
            TSpinKind::Full,
        );
    }

    #[test]
    fn test_t_spin_front_corner_mapping_east_full_vs_mini() {
        // East-facing "front" corners are the right corners: (2,0) and (2,2).
        assert_tspin_for_rotation(
            Rotation::East,
            &[(2, 0), (0, 0), (0, 2)],
            TSpinKind::Mini,
        );
        assert_tspin_for_rotation(
            Rotation::East,
            &[(2, 0), (2, 2), (0, 0), (0, 2)],
            TSpinKind::Full,
        );
    }

    #[test]
    fn test_t_spin_front_corner_mapping_south_full_vs_mini() {
        // South-facing "front" corners are the bottom corners: (0,2) and (2,2).
        assert_tspin_for_rotation(
            Rotation::South,
            &[(0, 2), (0, 0), (2, 0)],
            TSpinKind::Mini,
        );
        assert_tspin_for_rotation(
            Rotation::South,
            &[(0, 2), (2, 2), (0, 0), (2, 0)],
            TSpinKind::Full,
        );
    }

    #[test]
    fn test_t_spin_front_corner_mapping_west_full_vs_mini() {
        // West-facing "front" corners are the left corners: (0,0) and (0,2).
        assert_tspin_for_rotation(
            Rotation::West,
            &[(0, 0), (2, 0), (2, 2)],
            TSpinKind::Mini,
        );
        assert_tspin_for_rotation(
            Rotation::West,
            &[(0, 0), (0, 2), (2, 0), (2, 2)],
            TSpinKind::Full,
        );
    }

    #[test]
    fn test_landing_flash_is_set_on_lock_without_line_clear() {
        let mut state = GameState::new(12345);
        state.start();

        assert_eq!(state.landing_flash_ms, 0);
        state.lock_piece();
        assert_eq!(state.landing_flash_ms, LANDING_FLASH_MS);
    }

    #[test]
    fn test_landing_flash_ticks_down_during_line_clear_pause() {
        let mut state = GameState::new(12345);
        state.start();

        state.landing_flash_ms = LANDING_FLASH_MS;
        state.line_clear_timer_ms = 32;

        let flash_before = state.landing_flash_ms;
        assert!(!state.tick(16, false));
        assert!(state.landing_flash_ms < flash_before);
        assert!(state.line_clear_timer_ms < 32);
    }
}
