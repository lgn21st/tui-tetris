//! Terminal Tetris runner (default binary).
//!
//! This is the primary gameplay entrypoint.
//! It uses crossterm for input and a custom framebuffer-based renderer
//! (no ratatui widgets/layout).

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use tui_tetris::adapter::{Adapter, OutboundMessage};
use tui_tetris::core::{GameSnapshot, GameState};
use tui_tetris::engine::place::{apply_place, PlaceError};
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
    let mut snap = GameSnapshot::default();
    let mut last_board_id = game_state.board_id();
    game_state.snapshot_board_into(&mut snap);

    let mut adapter = Adapter::start_from_env();
    let obs_interval_ms: u32 = std::env::var("TETRIS_AI_OBS_HZ")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .map(|hz| (1000 / hz.clamp(1, 60)).max(1))
        .unwrap_or(50); // 20Hz default
    let mut obs_accum_ms: u32 = 0;
    let mut obs_seq: u64 = 0;

    // Observation meta tracking.
    let mut last_episode_id = game_state.episode_id();
    let mut last_piece_id = game_state.piece_id();
    let mut last_active_id = game_state.active_id();
    let mut last_paused = game_state.paused();
    let mut last_game_over = game_state.game_over();
    let mut pending_last_event: Option<tui_tetris::adapter::protocol::LastEvent> = None;

    let mut last_tick = Instant::now();
    let tick_duration = Duration::from_millis(TICK_MS as u64);
    let mut soft_drop_active = false;
    let mut soft_drop_timer_ms: i32 = SOFT_DROP_GRACE_MS as i32;

    loop {
        // Render.
        let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
        if game_state.board_id() != last_board_id {
            last_board_id = game_state.board_id();
            game_state.snapshot_board_into(&mut snap);
        }
        game_state.snapshot_meta_into(&mut snap);
        view.render_into(&snap, Viewport::new(w, h), &mut fb);
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

                    match cmd.payload {
                        tui_tetris::adapter::runtime::InboundPayload::SnapshotRequest => {
                            // Send an immediate observation to this client.
                            obs_seq = obs_seq.wrapping_add(1);
                            if game_state.board_id() != last_board_id {
                                last_board_id = game_state.board_id();
                                game_state.snapshot_board_into(&mut snap);
                            }
                            game_state.snapshot_meta_into(&mut snap);
                            let obs = tui_tetris::adapter::server::build_observation(
                                obs_seq,
                                &snap,
                                pending_last_event.take(),
                            );
                            ad.send(OutboundMessage::ToClientObservation {
                                client_id: cmd.client_id,
                                obs,
                            });
                            continue;
                        }
                        tui_tetris::adapter::runtime::InboundPayload::Command(cmd2) => {
                            let ok: Result<(), PlaceError> = match cmd2 {
                                tui_tetris::adapter::runtime::ClientCommand::Actions(actions) => {
                                    for a in actions {
                                        let _ = game_state.apply_action(a);
                                    }
                                    Ok(())
                                }
                                tui_tetris::adapter::runtime::ClientCommand::Place {
                                    x,
                                    rotation,
                                    use_hold,
                                } => apply_place(&mut game_state, x, rotation, use_hold),
                            };

                            // If applying a command caused a lock/clear event, mark it for immediate observation.
                            if let Some(ev) = game_state.take_last_event() {
                                pending_last_event =
                                    Some(tui_tetris::adapter::protocol::LastEvent {
                                        locked: ev.locked,
                                        lines_cleared: ev.lines_cleared,
                                        line_clear_score: ev.line_clear_score,
                                        tspin: ev.tspin.and_then(|t| match t {
                                            tui_tetris::types::TSpinKind::Mini => Some(
                                                tui_tetris::adapter::protocol::TSpinLower::Mini,
                                            ),
                                            tui_tetris::types::TSpinKind::Full => Some(
                                                tui_tetris::adapter::protocol::TSpinLower::Full,
                                            ),
                                            tui_tetris::types::TSpinKind::None => None,
                                        }),
                                        combo: ev.combo,
                                        back_to_back: ev.back_to_back,
                                    });
                            }

                            // Ack/error after apply.
                            match ok {
                                Ok(()) => {
                                    let ack =
                                        tui_tetris::adapter::protocol::create_ack(cmd.seq, cmd.seq);
                                    ad.send(OutboundMessage::ToClientAck {
                                        client_id: cmd.client_id,
                                        ack,
                                    });
                                }
                                Err(e) => {
                                    let code = match e.code() {
                                        "hold_unavailable" => {
                                            tui_tetris::adapter::protocol::ErrorCode::HoldUnavailable
                                        }
                                        "invalid_place" => {
                                            tui_tetris::adapter::protocol::ErrorCode::InvalidPlace
                                        }
                                        _ => tui_tetris::adapter::protocol::ErrorCode::InvalidCommand,
                                    };
                                    let err = tui_tetris::adapter::protocol::create_error(
                                        cmd.seq,
                                        code,
                                        e.message(),
                                    );
                                    ad.send(OutboundMessage::ToClientError {
                                        client_id: cmd.client_id,
                                        err,
                                    });
                                }
                            }
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

            // Detect piece changes via core piece_id.
            if game_state.piece_id() != last_piece_id {
                last_piece_id = game_state.piece_id();
                critical = true;
            }

            // Detect active-instance changes (e.g. hold swaps) and flush immediately.
            if game_state.active_id() != last_active_id {
                last_active_id = game_state.active_id();
                critical = true;
            }

            if game_state.paused() != last_paused {
                last_paused = game_state.paused();
                critical = true;
            }
            if game_state.game_over() != last_game_over {
                last_game_over = game_state.game_over();
                critical = true;
            }

            if game_state.episode_id() != last_episode_id {
                last_episode_id = game_state.episode_id();
                critical = true;
            }

            // Pull core last-event (accurate lock/clear).
            if let Some(ev) = game_state.take_last_event() {
                pending_last_event = Some(tui_tetris::adapter::protocol::LastEvent {
                    locked: ev.locked,
                    lines_cleared: ev.lines_cleared,
                    line_clear_score: ev.line_clear_score,
                    tspin: ev.tspin.and_then(|t| match t {
                        tui_tetris::types::TSpinKind::Mini => {
                            Some(tui_tetris::adapter::protocol::TSpinLower::Mini)
                        }
                        tui_tetris::types::TSpinKind::Full => {
                            Some(tui_tetris::adapter::protocol::TSpinLower::Full)
                        }
                        tui_tetris::types::TSpinKind::None => None,
                    }),
                    combo: ev.combo,
                    back_to_back: ev.back_to_back,
                });
                critical = true;
            }

            obs_accum_ms = obs_accum_ms.saturating_add(TICK_MS);
            if critical || obs_accum_ms >= obs_interval_ms {
                obs_accum_ms = 0;
                obs_seq = obs_seq.wrapping_add(1);

                let last_event = pending_last_event.take();

                if let Some(ad) = adapter.as_ref() {
                    if game_state.board_id() != last_board_id {
                        last_board_id = game_state.board_id();
                        game_state.snapshot_board_into(&mut snap);
                    }
                    game_state.snapshot_meta_into(&mut snap);
                    let obs =
                        tui_tetris::adapter::server::build_observation(obs_seq, &snap, last_event);
                    ad.send(OutboundMessage::BroadcastObservation { obs });
                }
            }
        }
    }
}
