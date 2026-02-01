//! TUI Tetris - Main entry point
//!
//! High-performance terminal Tetris game with AI control support.

use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Widget},
    Frame, Terminal,
};

use tui_tetris::core::GameState;
use tui_tetris::types::{GameAction, SOFT_DROP_GRACE_MS};
use tui_tetris::ui::{
    handle_key_event, render_game_over_overlay, render_pause_overlay, render_side_panel,
    should_quit, IncrementalRenderer, InputHandler,
};

/// Game tick interval (16ms = ~60 FPS)
const TICK_MS: u32 = 16;

fn main() -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Initialize game
    let mut game_state = GameState::new(1);
    game_state.start();

    // Game loop timing
    let mut last_tick = Instant::now();
    let tick_duration = Duration::from_millis(TICK_MS as u64);
    let mut soft_drop_active = false;
    let mut soft_drop_timer_ms: i32 = SOFT_DROP_GRACE_MS as i32;

    // Run game loop
    let result = run_game_loop(
        &mut terminal,
        &mut game_state,
        &mut last_tick,
        tick_duration,
        &mut soft_drop_active,
        &mut soft_drop_timer_ms,
    );

    // Restore terminal
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_game_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    game_state: &mut GameState,
    last_tick: &mut Instant,
    tick_duration: Duration,
    soft_drop_active: &mut bool,
    soft_drop_timer_ms: &mut i32,
) -> io::Result<()> {
    // Create incremental renderer (maintains state between frames)
    let mut renderer = IncrementalRenderer::new();

    // Create input handler for DAS/ARR support
    let mut input_handler = InputHandler::new();

    loop {
        // Draw UI using incremental renderer
        terminal.draw(|f| {
            draw_ui(f, game_state, &mut renderer);
        })?;

        // Handle input with timeout
        let timeout = tick_duration
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.kind {
                    KeyEventKind::Press => {
                        // Check quit
                        if should_quit(key) {
                            return Ok(());
                        }

                        // Handle DAS/ARR for movement keys
                        if let Some(action) = input_handler.handle_key_press(key.code) {
                            match action {
                                GameAction::SoftDrop => {
                                    *soft_drop_active = true;
                                    *soft_drop_timer_ms = SOFT_DROP_GRACE_MS as i32;
                                    game_state.apply_action(action);
                                }
                                _ => {
                                    game_state.apply_action(action);
                                }
                            }
                        }

                        // Handle other game actions (non-DAS keys)
                        if let Some(action) = handle_key_event(key) {
                            match action {
                                GameAction::MoveLeft | GameAction::MoveRight => {
                                    // These are handled by input_handler
                                }
                                GameAction::SoftDrop => {
                                    // Already handled above
                                }
                                _ => {
                                    game_state.apply_action(action);
                                }
                            }
                        }
                    }
                    KeyEventKind::Release => {
                        // Handle key release for DAS/ARR
                        input_handler.handle_key_release(key.code);
                    }
                    _ => {}
                }
            }
        }

        // Update game timing
        if last_tick.elapsed() >= tick_duration {
            *last_tick = Instant::now();

            // Get DAS/ARR auto-repeat actions
            let auto_actions = input_handler.update(TICK_MS);
            for action in auto_actions {
                match action {
                    GameAction::SoftDrop => {
                        *soft_drop_active = true;
                        *soft_drop_timer_ms = SOFT_DROP_GRACE_MS as i32;
                        game_state.apply_action(action);
                    }
                    _ => {
                        game_state.apply_action(action);
                    }
                }
            }

            // Update soft drop timer
            if *soft_drop_active {
                *soft_drop_timer_ms -= TICK_MS as i32;
                if *soft_drop_timer_ms <= 0 {
                    *soft_drop_active = false;
                }
            }

            // Tick game state
            game_state.tick(TICK_MS, *soft_drop_active);

            // Check game over
            if game_state.game_over {
                // Reset input handler on game over
                input_handler.reset();

                // Show game over screen and wait for input
                terminal.draw(|f| {
                    draw_ui(f, game_state, &mut renderer);
                    render_game_over_overlay(f.area(), f.buffer_mut());
                })?;

                // Wait for restart or quit
                loop {
                    if let Event::Key(key) = event::read()? {
                        if key.kind == KeyEventKind::Press {
                            if should_quit(key) {
                                return Ok(());
                            }
                            if let Some(GameAction::Restart) = handle_key_event(key) {
                                *game_state = GameState::new(1);
                                game_state.start();
                                *soft_drop_active = false;
                                *soft_drop_timer_ms = SOFT_DROP_GRACE_MS as i32;
                                input_handler.reset();
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn draw_ui(f: &mut Frame, game_state: &GameState, renderer: &mut IncrementalRenderer) {
    let area = f.area();

    // Clear background
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );

    // Split layout: board on left, panel on right
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let board_area = chunks[0];
    let panel_area = chunks[1];

    // Render board using incremental renderer
    renderer.render(game_state, board_area, f.buffer_mut());

    // Render side panel
    render_side_panel(game_state, panel_area, f.buffer_mut());

    // Render pause overlay if paused
    if game_state.paused {
        render_pause_overlay(area, f.buffer_mut());
    }
}
