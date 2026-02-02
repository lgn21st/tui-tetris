//! Terminal Tetris runner (default binary).
//!
//! This is the primary gameplay entrypoint.
//! It uses crossterm for input and a custom framebuffer-based renderer
//! (no ratatui widgets/layout).

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use tui_tetris::adapter::{Adapter, OutboundMessage};
use tui_tetris::core::GameState;
use tui_tetris::input::{handle_key_event, should_quit, InputHandler};
use tui_tetris::term::{GameView, TerminalRenderer, Viewport};
use tui_tetris::types::{GameAction, SOFT_DROP_GRACE_MS, TICK_MS};

fn main() -> Result<()> {
    let mut term = TerminalRenderer::new();
    term.enter()?;

    let result = run(&mut term);

    // Always try to restore terminal state.
    let _ = term.exit();
    result
}

fn run(term: &mut TerminalRenderer) -> Result<()> {
    let mut game_state = GameState::new(1);
    game_state.start();

    let view = GameView::default();
    let mut fb = tui_tetris::term::FrameBuffer::new(80, 24);
    let mut input_handler = InputHandler::new();

    let mut adapter = Adapter::start_from_env();
    let obs_interval_ms: u32 = std::env::var("TETRIS_AI_OBS_HZ")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .map(|hz| (1000 / hz.clamp(1, 60)).max(1))
        .unwrap_or(50); // 20Hz default
    let mut obs_accum_ms: u32 = 0;
    let mut obs_seq: u64 = 0;

    // Observation meta tracking.
    let episode_id: u32 = 0;
    let mut piece_id: u32 = 0;
    let mut step_in_piece: u32 = 0;
    let mut last_active_kind = game_state.active.map(|p| p.kind);
    let mut last_paused = game_state.paused;
    let mut last_game_over = game_state.game_over;
    let mut last_lines = game_state.lines;
    let mut last_score = game_state.score;
    let mut last_filled = board_filled_count(&game_state);

    let mut last_tick = Instant::now();
    let tick_duration = Duration::from_millis(TICK_MS as u64);
    let mut soft_drop_active = false;
    let mut soft_drop_timer_ms: i32 = SOFT_DROP_GRACE_MS as i32;

    loop {
        // Render.
        let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
        view.render_into(&game_state, Viewport::new(w, h), &mut fb);
        term.draw_swap(&mut fb)?;

        // Input with timeout until next tick.
        let timeout = tick_duration
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            match event::read()? {
                Event::Resize(_, _) => {
                    // Ensure next frame does a full redraw.
                    term.invalidate();
                }
                Event::Key(key) => match key.kind {
                    KeyEventKind::Press => {
                        if should_quit(key) {
                            return Ok(());
                        }

                        if let Some(action) = input_handler.handle_key_press(key.code) {
                            match action {
                                GameAction::SoftDrop => {
                                    soft_drop_active = true;
                                    soft_drop_timer_ms = SOFT_DROP_GRACE_MS as i32;
                                    game_state.apply_action(action);
                                }
                                _ => {
                                    game_state.apply_action(action);
                                }
                            }
                        }

                        if let Some(action) = handle_key_event(key) {
                            match action {
                                GameAction::MoveLeft
                                | GameAction::MoveRight
                                | GameAction::SoftDrop => {
                                    // Handled by input_handler / soft drop above.
                                }
                                _ => {
                                    game_state.apply_action(action);
                                }
                            }
                        }
                    }
                    KeyEventKind::Repeat => {
                        // Ignore terminal auto-repeat; DAS/ARR handles repeats internally.
                    }
                    KeyEventKind::Release => {
                        input_handler.handle_key_release(key.code);
                    }
                },
                _ => {}
            }
        }

        // Tick.
        if last_tick.elapsed() >= tick_duration {
            last_tick = Instant::now();

            // Apply AI commands before tick (determinism).
            if let Some(ad) = adapter.as_mut() {
                // Prevent pathological command floods.
                for _ in 0..32 {
                    let Some(cmd) = ad.try_recv() else { break };

                    let mut ok = true;
                    match cmd.command {
                        tui_tetris::adapter::runtime::ClientCommand::Actions(actions) => {
                            for a in actions {
                                let _ = game_state.apply_action(a);
                            }
                        }
                        tui_tetris::adapter::runtime::ClientCommand::Place {
                            x,
                            rotation,
                            use_hold,
                        } => {
                            ok = apply_place(&mut game_state, x, rotation, use_hold).is_ok();
                        }
                    }

                    // Ack/error after apply.
                    if ok {
                        let ack = tui_tetris::adapter::protocol::create_ack(cmd.seq, cmd.seq);
                        if let Ok(line) = serde_json::to_string(&ack) {
                            ad.send(OutboundMessage::ToClient {
                                client_id: cmd.client_id,
                                line,
                            });
                        }
                    } else {
                        let err = tui_tetris::adapter::protocol::create_error(
                            cmd.seq,
                            "invalid_place",
                            "place command could not be applied",
                        );
                        if let Ok(line) = serde_json::to_string(&err) {
                            ad.send(OutboundMessage::ToClient {
                                client_id: cmd.client_id,
                                line,
                            });
                        }
                    }
                }
            }

            for action in input_handler.update(TICK_MS) {
                match action {
                    GameAction::SoftDrop => {
                        soft_drop_active = true;
                        soft_drop_timer_ms = SOFT_DROP_GRACE_MS as i32;
                        game_state.apply_action(action);
                    }
                    _ => {
                        game_state.apply_action(action);
                    }
                }
            }

            if soft_drop_active {
                soft_drop_timer_ms -= TICK_MS as i32;
                if soft_drop_timer_ms <= 0 {
                    soft_drop_active = false;
                }
            }

            game_state.tick(TICK_MS, soft_drop_active);

            // Observation scheduling (20Hz + immediate on critical events).
            let mut critical = false;

            let prev_active_kind = last_active_kind;

            // Detect piece changes.
            let active_kind = game_state.active.map(|p| p.kind);
            if active_kind != last_active_kind {
                piece_id = piece_id.wrapping_add(1);
                step_in_piece = 0;
                last_active_kind = active_kind;
                critical = true;
            } else {
                step_in_piece = step_in_piece.wrapping_add(1);
            }

            if game_state.paused != last_paused {
                last_paused = game_state.paused;
                critical = true;
            }
            if game_state.game_over != last_game_over {
                last_game_over = game_state.game_over;
                critical = true;
            }

            // Board/score/lines changes imply a meaningful update (lock/clear).
            let filled = board_filled_count(&game_state);
            let board_changed = filled != last_filled;
            if board_changed {
                last_filled = filled;
                critical = true;
            }

            let lines_delta = game_state.lines.saturating_sub(last_lines);
            if lines_delta != 0 {
                last_lines = game_state.lines;
                critical = true;
            }
            let score_delta = game_state.score.saturating_sub(last_score);
            if score_delta != 0 {
                last_score = game_state.score;
            }

            // Heuristic: a lock event is a piece change where the board/score/lines changed.
            let locked_event = prev_active_kind.is_some()
                && active_kind != prev_active_kind
                && (board_changed || lines_delta != 0 || score_delta != 0);

            obs_accum_ms = obs_accum_ms.saturating_add(TICK_MS);
            if critical || obs_accum_ms >= obs_interval_ms {
                obs_accum_ms = 0;
                obs_seq = obs_seq.wrapping_add(1);

                let last_event = if locked_event || lines_delta != 0 {
                    Some(tui_tetris::adapter::protocol::LastEvent {
                        locked: locked_event,
                        lines_cleared: lines_delta,
                        line_clear_score: if lines_delta > 0 { score_delta } else { 0 },
                        tspin: None,
                        combo: game_state.combo,
                        back_to_back: game_state.back_to_back,
                    })
                } else {
                    None
                };

                if let Some(ad) = adapter.as_ref() {
                    let obs = tui_tetris::adapter::server::build_observation(
                        &game_state,
                        obs_seq,
                        episode_id,
                        piece_id,
                        step_in_piece,
                        last_event,
                    );
                    if let Ok(line) = serde_json::to_string(&obs) {
                        ad.send(OutboundMessage::Broadcast { line });
                    }
                }
            }
        }
    }
}

fn apply_place(
    state: &mut GameState,
    target_x: i8,
    target_rot: tui_tetris::types::Rotation,
    use_hold: bool,
) -> Result<(), ()> {
    if use_hold {
        state.apply_action(GameAction::Hold);
    }

    let Some(mut active) = state.active else {
        return Err(());
    };

    // Rotate to desired rotation (CW only for now).
    for _ in 0..4 {
        if active.rotation == target_rot {
            break;
        }
        if !state.try_rotate(true) {
            return Err(());
        }
        active = state.active.unwrap();
    }

    if active.rotation != target_rot {
        return Err(());
    }

    let dx = target_x - active.x;
    if dx > 0 {
        for _ in 0..dx {
            if !state.try_move(1, 0) {
                return Err(());
            }
        }
    } else if dx < 0 {
        for _ in 0..(-dx) {
            if !state.try_move(-1, 0) {
                return Err(());
            }
        }
    }

    state.apply_action(GameAction::HardDrop);
    Ok(())
}

fn board_filled_count(state: &GameState) -> u16 {
    state.board.cells().iter().filter(|c| c.is_some()).count() as u16
}
