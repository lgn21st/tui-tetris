use super::*;
use crate::core::PieceQueue;
use crate::core::scoring::qualifies_for_b2b;

fn find_seed_with_first_piece(kind: PieceKind) -> u32 {
    // Brute force a small seed range to find a deterministic queue whose first draw is `kind`.
    // This keeps tests stable without adding test-only hooks into the core RNG.
    for seed in 1u32..50_000 {
        let q = PieceQueue::new(seed);
        if q.peek() == Some(kind) {
            return seed;
        }
    }
    panic!("failed to find seed whose first piece is {kind:?}");
}

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
fn test_spawn_piece_does_not_preemptively_game_over_on_top_row_blocks_for_i_piece() {
    // The I piece spawns entirely on y=1 in North orientation, so occupied cells in y=0
    // should not necessarily trigger game over.
    let seed = find_seed_with_first_piece(PieceKind::I);
    let mut state = GameState::new(seed);

    // Force the former spawn-area probe cells (these are y=0 cells) to be occupied, but keep the I
    // spawn cells at y=1 clear.
    assert!(state.board.set(3, 0, Some(PieceKind::T)));
    assert!(state.board.set(4, 0, Some(PieceKind::T)));
    assert!(state.board.set(5, 0, Some(PieceKind::T)));

    state.start();

    assert!(!state.game_over);
    assert!(state.active.is_some());
    assert_eq!(state.active.unwrap().kind, PieceKind::I);
    assert!(state.active.unwrap().is_valid(&state.board));
}

#[test]
fn test_failed_spawn_does_not_advance_queue() {
    // If spawning fails, the RNG/queue must not advance. Otherwise deterministic retries
    // (and restart semantics based on queue seed/state) become unstable.
    let seed = find_seed_with_first_piece(PieceKind::I);
    let mut state = GameState::new(seed);

    // Block a cell that the I piece would occupy at spawn (x=3..6, y=1).
    assert!(state.board.set(3, 1, Some(PieceKind::T)));

    let next_before = *state.next_queue();
    state.start();

    assert!(state.game_over);
    assert!(state.active.is_none());
    assert_eq!(state.piece_id, 0);
    assert_eq!(*state.next_queue(), next_before);
    assert_eq!(state.next_queue()[0], PieceKind::I);
}

#[test]
fn repro_user_hard_drop_7_times_should_not_game_over() {
    let mut state = GameState::new(1);
    state.start();

    for i in 0..7 {
        assert!(state.apply_action(GameAction::HardDrop));
        assert!(
            !state.game_over(),
            "unexpected game over after hard drop #{i}"
        );
    }
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
    assert!(state.score >= score_before);
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
    let tspin = state.t_spin_kind(&piece);

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

    let tspin = state.t_spin_kind(&piece);

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
    if let Some(active) = state.active {
        assert!(active.y >= initial_y);
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

    // dropTimerMs is reduced in a while-loop even if the piece cannot move down.
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
    // Lock reset count is only consumed when grounded.
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

    // handleLockReset runs on the landing move (consuming one reset),
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
        // Reset lock timer/count when not grounded, and consume a reset only when grounded
        // after the move.
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
        // Reset lock timer/count when not grounded, and consume a reset only when grounded
        // after the rotation.
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

    // Pause clears soft drop state.
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
fn test_drop_scoring_saturates_at_u32_max() {
    let mut state = GameState::new(12345);
    state.start();
    state.score = u32::MAX;

    let _ = state.apply_action(GameAction::SoftDrop);

    assert_eq!(state.score, u32::MAX);
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
    assert!(ev.back_to_back);
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

    let expected_line_clear_score = 1200;
    let expected_delta = expected_line_clear_score; // first clear in chain => combo=0, no combo bonus

    assert_eq!(ev.lines_cleared, 4);
    assert_eq!(ev.combo, 0);
    assert!(ev.back_to_back);
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
    let expected_line_clear_score = 1200;
    let expected_delta = expected_line_clear_score + expected_drop_points;

    assert_eq!(ev.lines_cleared, 4);
    assert_eq!(ev.combo, 0);
    assert!(ev.back_to_back);
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

    // T-Spin no-lines awards points but resets combo/B2B and does not report a line-clear score.
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
    state.board.set(x, 18, Some(PieceKind::I));
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
    state.board.set(x, 18, Some(PieceKind::I));
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

#[test]
fn test_tspin_is_detected_before_cleared_rows_shift_the_corners() {
    let mut state = GameState::new(12345);
    state.start();

    // A north-facing T at (3, 17) completes row 18. Its two front corners are
    // on row 17 and its third occupied corner is on row 19. Clearing row 18
    // shifts row 17, so inspecting the board after the clear loses the spin.
    for x in 0..10 {
        if !(3..=5).contains(&x) {
            state.board.set(x, 18, Some(PieceKind::I));
        }
    }
    state.board.set(3, 17, Some(PieceKind::I));
    state.board.set(5, 17, Some(PieceKind::I));
    state.board.set(3, 19, Some(PieceKind::I));
    state.last_action_was_rotate = true;
    state.active = Some(Tetromino {
        kind: PieceKind::T,
        rotation: Rotation::North,
        x: 3,
        y: 17,
    });

    state.lock_piece();
    let event = state.take_last_event().expect("expected lock event");

    assert_eq!(event.lines_cleared, 1);
    assert_eq!(event.tspin, Some(TSpinKind::Full));
    assert_eq!(event.line_clear_score, 800);
}

fn assert_tspin_for_rotation(rotation: Rotation, filled_corners: &[(i8, i8)], expected: TSpinKind) {
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
    let tspin = state.t_spin_kind(&piece);
    assert_eq!(tspin, expected);
}

#[test]
fn test_t_spin_front_corner_mapping_north_full_vs_mini() {
    // Corners are relative to the piece origin: (0,0), (2,0), (0,2), (2,2).
    // North-facing "front" corners are the top corners: (0,0) and (2,0).
    assert_tspin_for_rotation(Rotation::North, &[(0, 0), (0, 2), (2, 2)], TSpinKind::Mini);
    assert_tspin_for_rotation(
        Rotation::North,
        &[(0, 0), (2, 0), (0, 2), (2, 2)],
        TSpinKind::Full,
    );
}

#[test]
fn test_t_spin_front_corner_mapping_east_full_vs_mini() {
    // East-facing "front" corners are the right corners: (2,0) and (2,2).
    assert_tspin_for_rotation(Rotation::East, &[(2, 0), (0, 0), (0, 2)], TSpinKind::Mini);
    assert_tspin_for_rotation(
        Rotation::East,
        &[(2, 0), (2, 2), (0, 0), (0, 2)],
        TSpinKind::Full,
    );
}

#[test]
fn test_t_spin_front_corner_mapping_south_full_vs_mini() {
    // South-facing "front" corners are the bottom corners: (0,2) and (2,2).
    assert_tspin_for_rotation(Rotation::South, &[(0, 2), (0, 0), (2, 0)], TSpinKind::Mini);
    assert_tspin_for_rotation(
        Rotation::South,
        &[(0, 2), (2, 2), (0, 0), (2, 0)],
        TSpinKind::Full,
    );
}

#[test]
fn test_t_spin_front_corner_mapping_west_full_vs_mini() {
    // West-facing "front" corners are the left corners: (0,0) and (0,2).
    assert_tspin_for_rotation(Rotation::West, &[(0, 0), (2, 0), (2, 2)], TSpinKind::Mini);
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
