//! Terminal Tetris runner (default binary).
//!
//! This is the primary gameplay entrypoint.
//! It uses crossterm for input and a custom framebuffer-based renderer
//! (no ratatui widgets/layout).

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

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
    let mut input_handler = InputHandler::new();

    let mut last_tick = Instant::now();
    let tick_duration = Duration::from_millis(TICK_MS as u64);
    let mut soft_drop_active = false;
    let mut soft_drop_timer_ms: i32 = SOFT_DROP_GRACE_MS as i32;

    loop {
        // Render.
        let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
        let fb = view.render(&game_state, Viewport::new(w, h));
        term.draw(&fb)?;

        // Input with timeout until next tick.
        let timeout = tick_duration
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.kind {
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
                }
            }
        }

        // Tick.
        if last_tick.elapsed() >= tick_duration {
            last_tick = Instant::now();

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
        }
    }
}
