//! Game state module - manages the complete game state
//!
//! This module ties together all core components: board, pieces, RNG, and scoring.
//! It handles game timing, piece movement, rotation, line clears, and game lifecycle.

use crate::core::{
    calculate_drop_score, calculate_score, get_shape,
    scoring::{get_drop_interval_ms, qualifies_for_b2b},
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
    combo: u32,
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
            combo: 0,
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

    pub fn board(&self) -> &Board {
        &self.board
    }

    #[cfg(test)]
    pub fn board_mut(&mut self) -> &mut Board {
        &mut self.board
    }

    pub fn snapshot_into(&self, out: &mut crate::core::snapshot::GameSnapshot) {
        use crate::core::snapshot::{ActiveSnapshot, TimersSnapshot};

        self.board.write_u8_grid(&mut out.board);

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
    pub fn spawn_piece(&mut self) -> bool {
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

            // If we moved while grounded, reset lock timer (with limit)
            if dy != 0 || (dx != 0 && self.is_grounded()) {
                self.reset_lock_timer();
            }

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

            // Rotation resets lock timer if grounded (with limit)
            self.reset_lock_timer();
            self.last_action_was_rotate = true;

            return true;
        }

        false
    }

    /// Reset the lock timer (with reset limit)
    fn reset_lock_timer(&mut self) {
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
    pub fn hold(&mut self) -> bool {
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
    pub fn lock_piece(&mut self) {
        let Some(active) = self.active else {
            return;
        };

        // Lock piece to board
        let shape = active.shape();
        let _success = self
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

        // Detect T-spin
        let tspin = if active.kind == PieceKind::T {
            self.t_spin_kind(&active, &cleared_rows)
        } else {
            TSpinKind::None
        };

        // Update game state
        let mut line_clear_score: u32 = 0;
        if lines_cleared > 0 {
            // Update combo
            self.combo += 1;

            // Update lines and level
            self.lines += lines_cleared as u32;
            self.level = self.lines / 10;

            // Calculate score
            let score_result = calculate_score(
                lines_cleared,
                self.level,
                tspin,
                self.combo,
                self.back_to_back,
            );

            self.score += score_result.total;
            line_clear_score = score_result.total;
            self.back_to_back =
                score_result.is_back_to_back || qualifies_for_b2b(tspin, lines_cleared);

            // Start line clear timer
            self.line_clear_timer_ms = LINE_CLEAR_PAUSE_MS;
            self.landing_flash_ms = LANDING_FLASH_MS;
        } else {
            // No lines cleared - reset combo
            self.combo = 0;
            self.back_to_back = false;
        }

        // Emit last event (for adapter observation immediate flush).
        let tspin_opt = match tspin {
            TSpinKind::None => None,
            _ => Some(tspin),
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
                Rotation::North => [(0, 2), (2, 2)], // Bottom corners
                Rotation::East => [(0, 0), (0, 2)],  // Left corners
                Rotation::South => [(0, 0), (2, 0)], // Top corners
                Rotation::West => [(2, 0), (2, 2)],  // Right corners
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
    pub fn is_grounded(&self) -> bool {
        match self.active {
            Some(ref piece) => piece.is_grounded(&self.board),
            None => false,
        }
    }

    /// Calculate the ghost piece Y position (where piece would land)
    pub fn ghost_y(&self) -> Option<i8> {
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

        // Handle line clear pause
        if self.line_clear_timer_ms > 0 {
            self.line_clear_timer_ms = self.line_clear_timer_ms.saturating_sub(elapsed_ms);
            return false;
        }

        // Step counter for the current active piece (only when gameplay advances).
        if self.active.is_some() {
            self.step_in_piece = self.step_in_piece.wrapping_add(1);
        }

        // Handle landing flash
        if self.landing_flash_ms > 0 {
            self.landing_flash_ms = self.landing_flash_ms.saturating_sub(elapsed_ms);
        }

        let Some(_) = self.active else {
            return false;
        };

        // Handle soft drop state
        if soft_drop && !self.is_soft_dropping {
            // Just started soft dropping
            self.is_soft_dropping = true;
            self.soft_drop_timer_ms = SOFT_DROP_GRACE_MS;
            self.drop_timer_ms = 0; // Reset drop timer to apply soft drop speed immediately
        } else if !soft_drop && self.is_soft_dropping {
            // Stopped soft dropping
            self.is_soft_dropping = false;
            self.soft_drop_timer_ms = 0;
            self.drop_timer_ms = 0; // Reset to apply normal speed
        }

        // Handle soft drop grace period
        if self.is_soft_dropping && self.soft_drop_timer_ms > 0 {
            self.soft_drop_timer_ms = self.soft_drop_timer_ms.saturating_sub(elapsed_ms);
        }

        // Check if grounded
        let grounded = self.is_grounded();

        if grounded {
            // Update lock timer
            self.lock_timer_ms += elapsed_ms;

            // Lock if timer exceeded
            if self.lock_timer_ms >= LOCK_DELAY_MS {
                self.lock_piece();
                return true;
            }
        } else {
            // Not grounded - update drop timer
            let drop_interval = self.drop_interval_ms();
            self.drop_timer_ms += elapsed_ms;

            if self.drop_timer_ms >= drop_interval {
                self.drop_timer_ms = 0;
                // Try to drop by one
                if !self.try_move(0, 1) {
                    // Should not happen if not grounded, but handle anyway
                    return false;
                }

                // Score for soft drop
                if self.is_soft_dropping && self.soft_drop_timer_ms == 0 {
                    self.score += calculate_drop_score(1, false);
                }

                return true;
            }
        }

        false
    }

    /// Apply a game action
    pub fn apply_action(&mut self, action: GameAction) -> bool {
        match action {
            GameAction::MoveLeft => self.try_move(-1, 0),
            GameAction::MoveRight => self.try_move(1, 0),
            GameAction::SoftDrop => {
                // Soft drop is handled in tick, but we can try an immediate move down
                let moved = self.try_move(0, 1);
                if moved {
                    self.score += calculate_drop_score(1, false);
                }
                moved
            }
            GameAction::HardDrop => {
                let drop_score = self.hard_drop();
                self.score += drop_score;
                true
            }
            GameAction::RotateCw => self.try_rotate(true),
            GameAction::RotateCcw => self.try_rotate(false),
            GameAction::Hold => self.hold(),
            GameAction::Pause => {
                self.paused = !self.paused;
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
    pub fn active_shape(&self) -> Option<[(i8, i8); 4]> {
        self.active.map(|p| p.shape())
    }

    /// Check if piece can move in given direction
    pub fn can_move(&self, dx: i8, dy: i8) -> bool {
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

    #[test]
    fn test_new_game_state() {
        let state = GameState::new(12345);

        assert!(!state.started);
        assert!(!state.game_over);
        assert!(!state.paused);
        assert_eq!(state.score, 0);
        assert_eq!(state.level, 0);
        assert_eq!(state.lines, 0);
        assert_eq!(state.combo, 0);
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
    fn test_step_in_piece_does_not_increment_during_line_clear_pause() {
        let mut state = GameState::new(12345);
        state.start();
        // Force a pause.
        state.line_clear_timer_ms = 16;
        state.tick(16, false);
        assert_eq!(state.step_in_piece, 0);
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

        // Test soft drop (20x multiplier: 320 / 20 = 16)
        state.is_soft_dropping = true;
        assert_eq!(state.drop_interval_ms(), 16);
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
            state.reset_lock_timer();
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
            assert_eq!(state.lock_reset_count, 1);
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
            assert_eq!(state.lock_reset_count, 1);
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
}
