//! Terminal Tetris runner (default binary).
//!
//! This is the primary gameplay entrypoint.
//! It uses crossterm for input and a custom framebuffer-based renderer
//! (no ratatui widgets/layout).

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use tui_tetris::adapter::game_loop::drain_commands;
use tui_tetris::adapter::observation_schedule::ObservationSchedule;
use tui_tetris::adapter::{Adapter, OutboundMessage};
use tui_tetris::core::{GameSnapshot, GameState};
use tui_tetris::input::{handle_key_event, should_quit, InputHandler};
use tui_tetris::observe::{
    connect_observer_with_retry, observe_status_lines, parse_observe_args,
    snapshot_from_observation, ObserveEvent, ObserveReconnectPolicy,
};
use tui_tetris::term::AdapterStatusView;
use tui_tetris::term::{
    AnchorY, CellStyle, GameView, RenderThrottle, Rgb, TerminalRenderer, Viewport,
};
use tui_tetris::types::{GameAction, TICK_MS};

const MAX_CATCH_UP_STEPS: u32 = 8;

fn fixed_steps_due(elapsed: Duration, tick: Duration) -> u32 {
    if tick.is_zero() {
        return 0;
    }
    (elapsed.as_nanos() / tick.as_nanos()).min(MAX_CATCH_UP_STEPS as u128) as u32
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(config) = parse_observe_args(&args)? {
        return run_observe(config);
    }

    if headless_enabled() {
        return run_headless();
    }

    let mut term = TerminalRenderer::new();
    term.enter()?;

    let result = run(&mut term);

    // Always try to restore terminal state.
    let _ = term.exit();
    result
}

fn run_observe(config: tui_tetris::observe::ObserveConfig) -> Result<()> {
    let reconnect_policy = ObserveReconnectPolicy::default();
    let (mut rx, first_obs) = connect_observer_with_retry(&config, reconnect_policy)?;

    let mut term = TerminalRenderer::new();
    term.enter()?;

    let result = (|| -> Result<()> {
        let view = game_view_from_env();
        let mut fb = tui_tetris::term::FrameBuffer::new(80, 24);
        let mut latest_obs = first_obs;
        let mut snap = latest_obs
            .as_ref()
            .map(snapshot_from_observation)
            .unwrap_or_else(GameSnapshot::default);
        let mut last_term_size: (u16, u16) = (0, 0);
        let mut dirty = true;

        loop {
            while let Ok(event) = rx.try_recv() {
                match event {
                    ObserveEvent::Observation(obs) => {
                        latest_obs = Some(obs.clone());
                        snap = snapshot_from_observation(&obs);
                        dirty = true;
                    }
                    ObserveEvent::Error(_) | ObserveEvent::Closed => {
                        match connect_observer_with_retry(&config, reconnect_policy) {
                            Ok((new_rx, first_obs_after_reconnect)) => {
                                rx = new_rx;
                                latest_obs = first_obs_after_reconnect;
                                if let Some(obs) = latest_obs.as_ref() {
                                    snap = snapshot_from_observation(obs);
                                }
                                dirty = true;
                            }
                            Err(e) => {
                                eprintln!("{e}");
                                return Ok(());
                            }
                        }
                    }
                    ObserveEvent::Welcome => {}
                }
            }

            let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
            if (w, h) != last_term_size {
                last_term_size = (w, h);
                term.invalidate();
                dirty = true;
            }

            if dirty {
                view.render_into(&snap, Viewport::new(w, h), &mut fb);
                let observe_label = CellStyle {
                    fg: Rgb::new(220, 220, 220),
                    bg: Rgb::new(0, 0, 0),
                    bold: true,
                    dim: false,
                };
                for (i, line) in observe_status_lines(&config, latest_obs.as_ref())
                    .iter()
                    .enumerate()
                {
                    let y = i as u16;
                    if y >= h {
                        break;
                    }
                    fb.put_str(0, y, line, observe_label);
                }
                term.draw_swap(&mut fb)?;
                dirty = false;
            }

            if event::poll(Duration::from_millis(16))? {
                match event::read()? {
                    Event::Resize(_, _) => {
                        term.invalidate();
                        dirty = true;
                    }
                    Event::Key(key) if key.kind == KeyEventKind::Press && should_quit(key) => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    })();

    let _ = term.exit();
    result
}

fn headless_enabled() -> bool {
    std::env::var("TUI_TETRIS_HEADLESS")
        .ok()
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn run_headless() -> Result<()> {
    let mut game_state = GameState::new(1);
    game_state.start();

    let mut snap = GameSnapshot::default();
    let mut last_board_id = game_state.board_id();
    game_state.snapshot_board_into(&mut snap);

    let mut adapter = Adapter::start_from_env()?;
    let mut adapter_streaming_count: u16 = 0;

    let mut observations = ObservationSchedule::from_env(&game_state);

    let mut last_tick = Instant::now();
    let tick_duration = Duration::from_millis(TICK_MS as u64);

    loop {
        // Drain adapter status updates to avoid unbounded growth.
        if let Some(ad) = adapter.as_mut() {
            while let Some(st) = ad.try_recv_status() {
                adapter_streaming_count = st.streaming_count;
            }
        }

        let steps = fixed_steps_due(last_tick.elapsed(), tick_duration);
        if steps == 0 {
            thread::sleep(tick_duration.saturating_sub(last_tick.elapsed()));
            continue;
        }

        for _ in 0..steps {
            last_tick += tick_duration;

            // Apply AI commands at the fixed-step boundary (determinism).
            drain_commands(
                &mut adapter,
                &mut game_state,
                &mut observations,
                &mut snap,
                &mut last_board_id,
            );

            game_state.tick(TICK_MS, false);

            if let Some((seq, last_event)) = observations.after_tick(&mut game_state) {
                if let Some(ad) = adapter.as_ref() {
                    if adapter_streaming_count == 0 {
                        continue;
                    }
                    if game_state.board_id() != last_board_id {
                        last_board_id = game_state.board_id();
                        game_state.snapshot_board_into(&mut snap);
                    }
                    game_state.snapshot_meta_into(&mut snap);
                    let obs =
                        tui_tetris::adapter::server::build_observation(seq, &snap, last_event);
                    ad.send(OutboundMessage::BroadcastObservationArc { obs: Arc::new(obs) });
                }
            }
        }
    }
}

fn run(term: &mut TerminalRenderer) -> Result<()> {
    let mut game_state = GameState::new(1);
    game_state.start();

    let view = game_view_from_env();
    let mut fb = tui_tetris::term::FrameBuffer::new(80, 24);
    let mut input_handler = InputHandler::new();
    if let Ok(s) = std::env::var("TUI_TETRIS_KEY_RELEASE_TIMEOUT_MS") {
        if let Ok(ms) = s.parse::<u32>() {
            input_handler = input_handler.with_key_release_timeout_ms(ms);
        }
    }
    // Optional: tune repeat-driven release bounds for terminals that emit Repeat but not Release.
    // Useful for Ghostty-like terminals that have repeat events but no key-up events.
    let repeat_min = std::env::var("TUI_TETRIS_REPEAT_RELEASE_TIMEOUT_MIN_MS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok());
    let repeat_max = std::env::var("TUI_TETRIS_REPEAT_RELEASE_TIMEOUT_MAX_MS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok());
    if let (Some(min_ms), Some(max_ms)) = (repeat_min, repeat_max) {
        input_handler = input_handler.with_repeat_release_timeout_bounds_ms(min_ms, max_ms);
    }
    let mut snap = GameSnapshot::default();
    let mut last_board_id = game_state.board_id();
    game_state.snapshot_board_into(&mut snap);
    let mut last_term_size: (u16, u16) = (0, 0);
    let render_epoch = Instant::now();
    let mut render_throttle = RenderThrottle::new(250);

    let mut adapter = Adapter::start_from_env()?;
    let listen_addr = if adapter.is_some() {
        adapter.as_ref().and_then(|a| a.listen_addr()).or_else(|| {
            // Fallback to configured env, mirroring adapter defaults.
            let host_s = std::env::var("TETRIS_AI_HOST")
                .ok()
                .unwrap_or_else(|| "127.0.0.1".to_string());
            let port = std::env::var("TETRIS_AI_PORT")
                .ok()
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(7777);
            host_s
                .trim()
                .parse::<std::net::IpAddr>()
                .ok()
                .map(|ip| std::net::SocketAddr::new(ip, port))
        })
    } else {
        None
    };
    let mut adapter_view = AdapterStatusView {
        enabled: adapter.is_some(),
        client_count: 0,
        controller_id: None,
        streaming_count: 0,
        pid: std::process::id(),
        listen_addr,
    };
    let mut observations = ObservationSchedule::from_env(&game_state);

    let mut last_tick = Instant::now();
    let tick_duration = Duration::from_millis(TICK_MS as u64);

    loop {
        // Drain adapter status updates.
        if let Some(ad) = adapter.as_mut() {
            while let Some(st) = ad.try_recv_status() {
                adapter_view.enabled = true;
                adapter_view.client_count = st.client_count;
                adapter_view.controller_id = st.controller_id;
                adapter_view.streaming_count = st.streaming_count;
            }
        }

        // Render (throttled while paused/game-over and unchanged).
        let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
        if (w, h) != last_term_size {
            last_term_size = (w, h);
            term.invalidate();
        }

        let now_ms = render_epoch.elapsed().as_millis() as u64;
        let is_static = game_state.paused() || game_state.game_over();
        let fingerprint = render_fingerprint(&game_state, &adapter_view, Viewport::new(w, h));

        if render_throttle.should_render(now_ms, fingerprint, is_static) {
            if game_state.board_id() != last_board_id {
                last_board_id = game_state.board_id();
                game_state.snapshot_board_into(&mut snap);
            }
            game_state.snapshot_meta_into(&mut snap);
            view.render_into_with_adapter(&snap, Some(&adapter_view), Viewport::new(w, h), &mut fb);
            term.draw_swap(&mut fb)?;
        }

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

                        // While paused/game over, input repeats are released and only Pause/Restart
                        // are accepted.
                        if game_state.paused() || game_state.game_over() {
                            input_handler.reset();
                            if let Some(action) = handle_key_event(key) {
                                match action {
                                    GameAction::Pause | GameAction::Restart => {
                                        let _ = game_state.apply_action(action);
                                    }
                                    _ => {}
                                }
                            }
                            continue;
                        }

                        if let Some(action) = input_handler.handle_key_press(key.code) {
                            game_state.apply_action(action);
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
                                    if matches!(action, GameAction::Pause | GameAction::Restart) {
                                        input_handler.reset();
                                    }
                                }
                            }
                        }
                    }
                    KeyEventKind::Repeat => {
                        if game_state.paused() || game_state.game_over() {
                            continue;
                        }

                        // Ignore terminal auto-repeat for actions, but treat movement repeats as
                        // key activity so the timeout-based auto-release doesn't fire while held.
                        match key.code {
                            crossterm::event::KeyCode::Left
                            | crossterm::event::KeyCode::Right
                            | crossterm::event::KeyCode::Down
                            | crossterm::event::KeyCode::Char('h')
                            | crossterm::event::KeyCode::Char('H')
                            | crossterm::event::KeyCode::Char('j')
                            | crossterm::event::KeyCode::Char('J')
                            | crossterm::event::KeyCode::Char('l')
                            | crossterm::event::KeyCode::Char('L')
                            | crossterm::event::KeyCode::Char('a')
                            | crossterm::event::KeyCode::Char('A')
                            | crossterm::event::KeyCode::Char('d')
                            | crossterm::event::KeyCode::Char('D')
                            | crossterm::event::KeyCode::Char('s')
                            | crossterm::event::KeyCode::Char('S') => {
                                input_handler.handle_key_repeat(key.code);
                            }
                            _ => {}
                        }
                    }
                    KeyEventKind::Release => {
                        if game_state.paused() || game_state.game_over() {
                            continue;
                        }
                        input_handler.handle_key_release(key.code);
                    }
                },
                _ => {}
            }
        }

        // Tick. Preserve accumulated wall time and cap work per loop so a
        // temporary stall does not permanently slow the deterministic simulation.
        let steps = fixed_steps_due(last_tick.elapsed(), tick_duration);
        for _ in 0..steps {
            last_tick += tick_duration;

            // Apply AI commands before tick (determinism).
            drain_commands(
                &mut adapter,
                &mut game_state,
                &mut observations,
                &mut snap,
                &mut last_board_id,
            );

            for action in input_handler.update(TICK_MS) {
                game_state.apply_action(action);
            }

            // Soft drop state is managed by core via the soft drop timeout.
            game_state.tick(TICK_MS, false);

            if let Some((seq, last_event)) = observations.after_tick(&mut game_state) {
                if let Some(ad) = adapter.as_ref() {
                    if adapter_view.streaming_count == 0 {
                        continue;
                    }
                    if game_state.board_id() != last_board_id {
                        last_board_id = game_state.board_id();
                        game_state.snapshot_board_into(&mut snap);
                    }
                    game_state.snapshot_meta_into(&mut snap);
                    let obs =
                        tui_tetris::adapter::server::build_observation(seq, &snap, last_event);
                    ad.send(OutboundMessage::BroadcastObservationArc { obs: Arc::new(obs) });
                }
            }
        }
    }
}

fn render_fingerprint(
    game_state: &GameState,
    adapter: &AdapterStatusView,
    viewport: Viewport,
) -> u64 {
    // FNV-1a 64-bit over render-relevant fields only.
    let mut hash = 0xcbf29ce484222325_u64;
    let mut push_u64 = |value: u64| {
        for byte in value.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x00000100000001B3);
        }
    };

    push_u64(viewport.width as u64);
    push_u64(viewport.height as u64);
    push_u64(game_state.board_id() as u64);
    push_u64(game_state.episode_id() as u64);
    push_u64(game_state.piece_id() as u64);
    push_u64(game_state.active_id() as u64);
    push_u64(game_state.step_in_piece() as u64);
    push_u64(game_state.score() as u64);
    push_u64(game_state.level() as u64);
    push_u64(game_state.lines() as u64);
    push_u64(game_state.can_hold() as u64);
    push_u64(game_state.paused() as u64);
    push_u64(game_state.game_over() as u64);
    push_u64(
        game_state
            .hold_piece()
            .map(|piece| piece as u64)
            .unwrap_or(u64::MAX),
    );
    for piece in game_state.next_queue().iter().copied() {
        push_u64(piece as u64);
    }

    push_u64(adapter.enabled as u64);
    push_u64(adapter.client_count as u64);
    push_u64(adapter.streaming_count as u64);
    push_u64(adapter.controller_id.unwrap_or(usize::MAX) as u64);
    push_u64(adapter.pid as u64);
    if let Some(addr) = adapter.listen_addr {
        push_u64(addr.ip().is_ipv4() as u64);
        match addr.ip() {
            std::net::IpAddr::V4(ip) => {
                for byte in ip.octets() {
                    push_u64(byte as u64);
                }
            }
            std::net::IpAddr::V6(ip) => {
                for segment in ip.segments() {
                    push_u64(segment as u64);
                }
            }
        }
        push_u64(addr.port() as u64);
    } else {
        push_u64(u64::MAX);
    }

    hash
}

fn game_view_from_env() -> GameView {
    let anchor_y = std::env::var("TUI_TETRIS_ANCHOR_Y")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
        .map(|v| match v {
            "top" => AnchorY::Top,
            "center" | "" => AnchorY::Center,
            _ => AnchorY::Center,
        })
        .unwrap_or(AnchorY::Center);
    GameView::default().with_anchor_y(anchor_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter_view() -> AdapterStatusView {
        AdapterStatusView {
            enabled: true,
            client_count: 0,
            controller_id: None,
            streaming_count: 0,
            pid: 1,
            listen_addr: None,
        }
    }

    #[test]
    fn render_fingerprint_changes_with_render_relevant_state() {
        let mut game = GameState::new(1);
        game.start();
        let adapter = adapter_view();
        let viewport = Viewport::new(80, 24);
        let before = render_fingerprint(&game, &adapter, viewport);

        game.tick(TICK_MS, false);

        assert_ne!(render_fingerprint(&game, &adapter, viewport), before);
    }

    #[test]
    fn render_fingerprint_includes_adapter_hud_and_viewport() {
        let mut game = GameState::new(1);
        game.start();
        let adapter = adapter_view();
        let baseline = render_fingerprint(&game, &adapter, Viewport::new(80, 24));

        let mut connected = adapter;
        connected.client_count = 1;

        assert_ne!(
            render_fingerprint(&game, &connected, Viewport::new(80, 24)),
            baseline
        );
        assert_ne!(
            render_fingerprint(&game, &adapter, Viewport::new(100, 30)),
            baseline
        );
    }

    #[test]
    fn fixed_step_catch_up_is_calculated_and_bounded_per_loop() {
        let tick = Duration::from_millis(TICK_MS as u64);

        assert_eq!(fixed_steps_due(Duration::from_millis(48), tick), 3);
        assert_eq!(
            fixed_steps_due(Duration::from_secs(1), tick),
            MAX_CATCH_UP_STEPS
        );
    }
}
