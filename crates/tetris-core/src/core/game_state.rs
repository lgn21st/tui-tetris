//! Game state module - manages the complete game state
//!
//! This module ties together all core components: board, pieces, RNG, and scoring.
//! It handles game timing, piece movement, rotation, line clears, and game lifecycle.

use crate::core::{
    Board, PieceQueue, calculate_drop_score, calculate_score, get_shape,
    scoring::get_drop_interval_ms, try_rotate,
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

    #[cfg(any(test, feature = "test-support"))]
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
        out.board_hash = {
            // FNV-1a 64-bit over the 20x10 cell grid (200 bytes), stored in the snapshot so
            // hot-path observation building can avoid re-hashing the board when it hasn't changed.
            let mut h: u64 = 0xcbf29ce484222325;
            for row in out.board.iter() {
                for b in row.iter() {
                    h ^= *b as u64;
                    h = h.wrapping_mul(0x00000100000001B3);
                }
            }
            h
        };
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
        // Peek the next piece and validate spawn before consuming it from the queue.
        //
        // This matters for determinism/restart semantics: if spawn fails and we set game_over,
        // we should not advance the RNG/queue state.
        let Some(kind) = self.piece_queue.peek() else {
            self.game_over = true;
            return false;
        };
        let piece = Tetromino::new(kind);

        // Verify spawn position is valid
        if !piece.is_valid(&self.board) {
            self.game_over = true;
            return false;
        }

        // Consume the validated piece.
        let drawn = self.piece_queue.draw();
        debug_assert_eq!(drawn, kind);

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

    /// Reset lock timers/counts semantics:
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
                if let Some(ref piece) = self.active
                    && !piece.is_valid(&self.board)
                {
                    self.game_over = true;
                    self.active = None;
                    return false;
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

        // Trigger landing flash on every lock.
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

        // Detect T-spin against the locked board before line clearing shifts the
        // corner cells away from the piece's lock position.
        let tspin = if lock_success && active.kind == PieceKind::T {
            self.t_spin_kind(&active)
        } else {
            TSpinKind::None
        };

        // Clear full rows
        let cleared_rows = self.board.clear_full_rows();
        let lines_cleared = cleared_rows.len();

        if lock_success || lines_cleared > 0 {
            self.board_id = self.board_id.wrapping_add(1);
        }

        // Update game state
        let line_clear_score = self.apply_line_clear(lines_cleared, tspin);

        // Emit last event (for adapter observation immediate flush).
        //
        // Only report the last T-Spin kind for line clears (reset after a lock with no clear),
        // so we only include `tspin` when at least one line was cleared.
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

            // Award points for T-Spin "no lines", but it does not count as a line clear for
            // combo/B2B/line_clear_score reporting.
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
    fn t_spin_kind(&self, piece: &Tetromino) -> TSpinKind {
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

        // Soft drop activation + timeout:
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

        // Gravity: accumulate and advance.
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
                // Soft drop step moves once (if possible), adds +1 score per cell,
                // and activates soft drop speed for a short grace window.
                let moved = self.try_move(0, 1);
                if moved {
                    self.score = self.score.saturating_add(calculate_drop_score(1, false));
                }
                self.is_soft_dropping = true;
                self.soft_drop_timer_ms = SOFT_DROP_GRACE_MS;
                self.last_action_was_rotate = false;
                moved
            }
            GameAction::HardDrop => {
                // Hard drop is an action, so it clears the rotate flag before lock.
                self.last_action_was_rotate = false;
                let drop_score = self.hard_drop();
                self.score = self.score.saturating_add(drop_score);
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
                let seed = self.piece_queue.rng_state();
                self.restart_with_seed(seed)
            }
        }
    }

    /// Restart the game with an explicit episode seed.
    ///
    /// This is used by the adapter protocol (`command(action restart)` with `restart.seed`)
    /// to guarantee determinism for training/evaluation.
    pub fn restart_with_seed(&mut self, seed: u32) -> bool {
        let next_episode = self.episode_id.wrapping_add(1);
        *self = Self::new(seed);
        self.episode_id = next_episode;
        self.start();
        true
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
#[path = "game_state_tests.rs"]
mod tests;
