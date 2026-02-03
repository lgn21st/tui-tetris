//! DAS/ARR input handler for terminal environments.
//!
//! Supports terminals that do not emit key release events by using a timeout.

use crossterm::event::KeyCode;

use arrayvec::ArrayVec;

use crate::types::{GameAction, DEFAULT_ARR_MS, DEFAULT_DAS_MS, SOFT_DROP_ARR_MS, SOFT_DROP_DAS_MS};

/// Direction for horizontal movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalDirection {
    Left,
    Right,
    None,
}

/// Tracks input state for DAS/ARR handling.
#[derive(Debug, Clone)]
pub struct InputHandler {
    horizontal: HorizontalDirection,
    down_held: bool,
    last_key_time: std::time::Instant,
    horizontal_das_timer: u32,
    down_das_timer: u32,
    horizontal_arr_accumulator: u32,
    down_arr_accumulator: u32,
    das_delay: u32,
    arr_rate: u32,
    key_release_timeout_ms: u32,
    saw_repeat_event: bool,
    last_repeat_time: Option<std::time::Instant>,
    repeat_release_timeout_ms: u32,
    repeat_release_timeout_min_ms: u32,
    repeat_release_timeout_max_ms: u32,
}

// In terminals without key-release events, a short timeout prevents a single tap
// from turning into a sustained "held" state that triggers DAS/ARR repeats.
const DEFAULT_KEY_RELEASE_TIMEOUT_MS: u32 = 150;
// When the terminal emits repeat events but not release events, we can treat the
// absence of repeats as a signal that the key was released. This shorter timeout
// prevents a released key from continuing to generate repeats for too long.
const MIN_REPEAT_DRIVEN_RELEASE_TIMEOUT_MS: u32 = 80;
const MAX_REPEAT_DRIVEN_RELEASE_TIMEOUT_MS: u32 = 300;

impl InputHandler {
    pub fn new() -> Self {
        Self::with_config(DEFAULT_DAS_MS, DEFAULT_ARR_MS)
    }

    pub fn with_config(das_delay: u32, arr_rate: u32) -> Self {
        Self {
            horizontal: HorizontalDirection::None,
            down_held: false,
            last_key_time: std::time::Instant::now(),
            horizontal_das_timer: 0,
            down_das_timer: 0,
            horizontal_arr_accumulator: 0,
            down_arr_accumulator: 0,
            das_delay,
            arr_rate,
            key_release_timeout_ms: DEFAULT_KEY_RELEASE_TIMEOUT_MS,
            saw_repeat_event: false,
            last_repeat_time: None,
            repeat_release_timeout_ms: MIN_REPEAT_DRIVEN_RELEASE_TIMEOUT_MS,
            repeat_release_timeout_min_ms: MIN_REPEAT_DRIVEN_RELEASE_TIMEOUT_MS,
            repeat_release_timeout_max_ms: MAX_REPEAT_DRIVEN_RELEASE_TIMEOUT_MS,
        }
    }

    pub fn with_key_release_timeout_ms(mut self, timeout_ms: u32) -> Self {
        self.key_release_timeout_ms = timeout_ms;
        self
    }

    pub fn key_release_timeout_ms(&self) -> u32 {
        self.key_release_timeout_ms
    }

    pub fn with_repeat_release_timeout_bounds_ms(mut self, min_ms: u32, max_ms: u32) -> Self {
        let min_ms = min_ms.max(1);
        let max_ms = max_ms.max(1);
        let (min_ms, max_ms) = if min_ms <= max_ms {
            (min_ms, max_ms)
        } else {
            (max_ms, min_ms)
        };
        self.repeat_release_timeout_min_ms = min_ms;
        self.repeat_release_timeout_max_ms = max_ms;
        self.repeat_release_timeout_ms =
            self.repeat_release_timeout_ms.clamp(min_ms, max_ms);
        self
    }

    pub fn handle_key_press(&mut self, code: KeyCode) -> Option<GameAction> {
        match code {
            KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('A') => {
                self.last_key_time = std::time::Instant::now();
                if self.horizontal == HorizontalDirection::Left {
                    // Some terminals (including those without key release events) may report
                    // key repeats as "press" events instead of "repeat" events. Treat this as
                    // repeat activity to enable repeat-driven auto-release behavior.
                    self.handle_key_repeat(code);
                    None
                } else {
                    self.horizontal = HorizontalDirection::Left;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                    Some(GameAction::MoveLeft)
                }
            }
            KeyCode::Char('h') | KeyCode::Char('H') => {
                self.last_key_time = std::time::Instant::now();
                if self.horizontal == HorizontalDirection::Left {
                    self.handle_key_repeat(code);
                    None
                } else {
                    self.horizontal = HorizontalDirection::Left;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                    Some(GameAction::MoveLeft)
                }
            }
            KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('D') => {
                self.last_key_time = std::time::Instant::now();
                if self.horizontal == HorizontalDirection::Right {
                    self.handle_key_repeat(code);
                    None
                } else {
                    self.horizontal = HorizontalDirection::Right;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                    Some(GameAction::MoveRight)
                }
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                self.last_key_time = std::time::Instant::now();
                if self.horizontal == HorizontalDirection::Right {
                    self.handle_key_repeat(code);
                    None
                } else {
                    self.horizontal = HorizontalDirection::Right;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                    Some(GameAction::MoveRight)
                }
            }
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') => {
                self.last_key_time = std::time::Instant::now();
                if self.down_held {
                    self.handle_key_repeat(code);
                    None
                } else {
                    self.down_held = true;
                    self.down_das_timer = 0;
                    self.down_arr_accumulator = 0;
                    Some(GameAction::SoftDrop)
                }
            }
            KeyCode::Char('j') | KeyCode::Char('J') => {
                self.last_key_time = std::time::Instant::now();
                if self.down_held {
                    self.handle_key_repeat(code);
                    None
                } else {
                    self.down_held = true;
                    self.down_das_timer = 0;
                    self.down_arr_accumulator = 0;
                    Some(GameAction::SoftDrop)
                }
            }
            _ => None,
        }
    }

    pub fn handle_key_repeat(&mut self, code: KeyCode) {
        let now = std::time::Instant::now();
        match code {
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Down
            | KeyCode::Char('a')
            | KeyCode::Char('A')
            | KeyCode::Char('d')
            | KeyCode::Char('D')
            | KeyCode::Char('s')
            | KeyCode::Char('S') => {
                self.last_key_time = now;
                self.saw_repeat_event = true;
            }
            KeyCode::Char('h')
            | KeyCode::Char('H')
            | KeyCode::Char('l')
            | KeyCode::Char('L')
            | KeyCode::Char('j')
            | KeyCode::Char('J') => {
                self.last_key_time = now;
                self.saw_repeat_event = true;
            }
            _ => {}
        }

        if self.saw_repeat_event {
            if let Some(prev) = self.last_repeat_time {
                let interval_ms = now.saturating_duration_since(prev).as_millis() as u32;
                let target = interval_ms.saturating_mul(2);
                self.repeat_release_timeout_ms = target
                    .clamp(self.repeat_release_timeout_min_ms, self.repeat_release_timeout_max_ms);
            }
            self.last_repeat_time = Some(now);
        }
    }

    pub fn handle_key_release(&mut self, code: KeyCode) {
        match code {
            KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('A') => {
                if self.horizontal == HorizontalDirection::Left {
                    self.horizontal = HorizontalDirection::None;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                }
            }
            KeyCode::Char('h') | KeyCode::Char('H') => {
                if self.horizontal == HorizontalDirection::Left {
                    self.horizontal = HorizontalDirection::None;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                }
            }
            KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('D') => {
                if self.horizontal == HorizontalDirection::Right {
                    self.horizontal = HorizontalDirection::None;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                }
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                if self.horizontal == HorizontalDirection::Right {
                    self.horizontal = HorizontalDirection::None;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                }
            }
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') => {
                self.down_held = false;
                self.down_das_timer = 0;
                self.down_arr_accumulator = 0;
            }
            KeyCode::Char('j') | KeyCode::Char('J') => {
                self.down_held = false;
                self.down_das_timer = 0;
                self.down_arr_accumulator = 0;
            }
            _ => {}
        }
    }

    pub fn update(&mut self, elapsed_ms: u32) -> ArrayVec<GameAction, 32> {
        let mut actions = ArrayVec::<GameAction, 32>::new();

        // Auto-release when terminal does not emit release events.
        let time_since_last_key = self.last_key_time.elapsed().as_millis() as u32;
        let timeout_ms = if self.saw_repeat_event {
            self.key_release_timeout_ms.min(self.repeat_release_timeout_ms)
        } else {
            self.key_release_timeout_ms
        };
        if time_since_last_key > timeout_ms {
            if self.horizontal != HorizontalDirection::None {
                self.horizontal = HorizontalDirection::None;
                self.horizontal_das_timer = 0;
                self.horizontal_arr_accumulator = 0;
            }
            if self.down_held {
                self.down_held = false;
                self.down_das_timer = 0;
                self.down_arr_accumulator = 0;
            }
        }

        match self.horizontal {
            HorizontalDirection::Left | HorizontalDirection::Right => {
                let prev_das = self.horizontal_das_timer;
                self.horizontal_das_timer += elapsed_ms;

                if self.horizontal_das_timer >= self.das_delay {
                    let excess = if prev_das < self.das_delay {
                        self.horizontal_das_timer - self.das_delay
                    } else {
                        elapsed_ms
                    };
                    self.horizontal_arr_accumulator += excess;

                    while self.horizontal_arr_accumulator >= self.arr_rate {
                        match self.horizontal {
                            HorizontalDirection::Left => {
                                let _ = actions.try_push(GameAction::MoveLeft);
                            }
                            HorizontalDirection::Right => {
                                let _ = actions.try_push(GameAction::MoveRight);
                            }
                            HorizontalDirection::None => {}
                        }
                        self.horizontal_arr_accumulator -= self.arr_rate;
                    }
                }
            }
            HorizontalDirection::None => {
                self.horizontal_das_timer = 0;
                self.horizontal_arr_accumulator = 0;
            }
        }

        if self.down_held {
            let prev_das = self.down_das_timer;
            self.down_das_timer += elapsed_ms;

            if self.down_das_timer >= SOFT_DROP_DAS_MS {
                let excess = if prev_das < SOFT_DROP_DAS_MS {
                    self.down_das_timer - SOFT_DROP_DAS_MS
                } else {
                    elapsed_ms
                };
                self.down_arr_accumulator += excess;
                while self.down_arr_accumulator >= SOFT_DROP_ARR_MS {
                    let _ = actions.try_push(GameAction::SoftDrop);
                    self.down_arr_accumulator -= SOFT_DROP_ARR_MS;
                }
            }
        } else {
            self.down_das_timer = 0;
            self.down_arr_accumulator = 0;
        }

        actions
    }

    pub fn reset(&mut self) {
        self.horizontal = HorizontalDirection::None;
        self.down_held = false;
        self.last_key_time = std::time::Instant::now();
        self.horizontal_das_timer = 0;
        self.down_das_timer = 0;
        self.horizontal_arr_accumulator = 0;
        self.down_arr_accumulator = 0;
        self.saw_repeat_event = false;
        self.last_repeat_time = None;
        self.repeat_release_timeout_ms = self.repeat_release_timeout_min_ms;
    }
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_horizontal_das_arr_repeats_after_delay() {
        let mut ih = InputHandler::with_config(100, 25);

        assert_eq!(ih.handle_key_press(KeyCode::Left), Some(GameAction::MoveLeft));

        // Before DAS expires: no repeats.
        let actions = ih.update(99);
        assert!(actions.is_empty());

        // Exactly at DAS: still no repeats (needs excess over DAS to accumulate ARR).
        let actions = ih.update(1);
        assert!(actions.is_empty());

        // First ARR interval after DAS: one repeat.
        let actions = ih.update(25);
        assert_eq!(actions.as_slice(), &[GameAction::MoveLeft]);

        // Another ARR interval: one repeat again.
        let actions = ih.update(25);
        assert_eq!(actions.as_slice(), &[GameAction::MoveLeft]);
    }

    #[test]
    fn test_auto_release_triggers_after_timeout_without_key_release_events() {
        let mut ih = InputHandler::with_config(100, 25);
        ih.key_release_timeout_ms = 50;

        assert_eq!(ih.handle_key_press(KeyCode::Left), Some(GameAction::MoveLeft));
        assert_eq!(ih.horizontal, HorizontalDirection::Left);

        // Simulate no key-release events by moving the last key time into the past.
        ih.last_key_time = std::time::Instant::now() - std::time::Duration::from_millis(51);

        let actions = ih.update(0);
        assert!(actions.is_empty());
        assert_eq!(ih.horizontal, HorizontalDirection::None);
    }

    #[test]
    fn test_non_movement_key_does_not_extend_auto_release_timeout() {
        let mut ih = InputHandler::with_config(100, 25);
        ih.key_release_timeout_ms = 50;

        assert_eq!(ih.handle_key_press(KeyCode::Left), Some(GameAction::MoveLeft));
        assert_eq!(ih.horizontal, HorizontalDirection::Left);

        // Simulate a stuck key (no release event) and then press a non-movement key.
        ih.last_key_time = std::time::Instant::now() - std::time::Duration::from_millis(51);
        assert_eq!(ih.handle_key_press(KeyCode::Up), None);

        // The stale movement key should still auto-release.
        let actions = ih.update(0);
        assert!(actions.is_empty());
        assert_eq!(ih.horizontal, HorizontalDirection::None);
    }

    #[test]
    fn test_default_key_release_timeout_is_non_zero() {
        let ih = InputHandler::new();
        assert!(ih.key_release_timeout_ms() > 0);
    }

    #[test]
    fn test_repeat_driven_auto_release_is_shorter_after_repeat_stops() {
        let mut ih = InputHandler::with_config(100, 25).with_key_release_timeout_ms(500);

        assert_eq!(ih.handle_key_press(KeyCode::Left), Some(GameAction::MoveLeft));
        assert_eq!(ih.horizontal, HorizontalDirection::Left);

        // Observing repeat events enables the shorter repeat-driven release timeout.
        ih.handle_key_repeat(KeyCode::Left);
        assert!(ih.saw_repeat_event);

        // Simulate an observed repeat cadence and ensure the computed timeout is used.
        ih.repeat_release_timeout_ms = 120;

        // Simulate repeats stopping (no release event): key should auto-release quickly.
        ih.last_key_time =
            std::time::Instant::now() - std::time::Duration::from_millis(121);

        let actions = ih.update(0);
        assert!(actions.is_empty());
        assert_eq!(ih.horizontal, HorizontalDirection::None);
    }

    #[test]
    fn test_terminals_that_report_repeat_as_press_enable_repeat_driven_release() {
        let mut ih = InputHandler::with_config(100, 25).with_key_release_timeout_ms(500);

        assert_eq!(ih.handle_key_press(KeyCode::Left), Some(GameAction::MoveLeft));
        assert!(!ih.saw_repeat_event);

        // Simulate a terminal reporting repeats as press events while the key is already held.
        assert_eq!(ih.handle_key_press(KeyCode::Left), None);
        assert!(ih.saw_repeat_event);

        // With repeat-driven mode on, the shorter timeout should apply.
        ih.repeat_release_timeout_ms = 80;
        ih.last_key_time = std::time::Instant::now() - std::time::Duration::from_millis(81);

        let _ = ih.update(0);
        assert_eq!(ih.horizontal, HorizontalDirection::None);
    }

    #[test]
    fn test_repeat_driven_timeout_does_not_break_slow_repeat_cadence() {
        let mut ih = InputHandler::with_config(100, 25).with_key_release_timeout_ms(500);
        assert_eq!(ih.handle_key_press(KeyCode::Left), Some(GameAction::MoveLeft));
        assert_eq!(ih.horizontal, HorizontalDirection::Left);

        ih.saw_repeat_event = true;
        ih.repeat_release_timeout_ms = 240;

        // A slower repeat cadence (e.g. 120ms) should not be treated as release.
        ih.last_key_time = std::time::Instant::now() - std::time::Duration::from_millis(120);
        let _ = ih.update(0);
        assert_eq!(ih.horizontal, HorizontalDirection::Left);

        // But exceeding the timeout should release.
        ih.last_key_time = std::time::Instant::now() - std::time::Duration::from_millis(241);
        let _ = ih.update(0);
        assert_eq!(ih.horizontal, HorizontalDirection::None);
    }

    #[test]
    fn test_repeat_release_timeout_bounds_can_be_overridden() {
        let ih = InputHandler::new().with_repeat_release_timeout_bounds_ms(200, 220);
        assert_eq!(ih.repeat_release_timeout_min_ms, 200);
        assert_eq!(ih.repeat_release_timeout_max_ms, 220);
        assert_eq!(ih.repeat_release_timeout_ms, 200);
    }

    #[test]
    fn test_soft_drop_repeats_use_zero_das_and_50ms_arr() {
        let mut ih = InputHandler::new().with_key_release_timeout_ms(10_000);

        assert_eq!(ih.handle_key_press(KeyCode::Down), Some(GameAction::SoftDrop));

        // Before 50ms: no repeats.
        let actions = ih.update(49);
        assert!(actions.is_empty());

        // At 50ms: exactly one repeat.
        let actions = ih.update(1);
        assert_eq!(actions.as_slice(), &[GameAction::SoftDrop]);

        // Another 100ms: two repeats.
        let actions = ih.update(100);
        assert_eq!(
            actions.as_slice(),
            &[GameAction::SoftDrop, GameAction::SoftDrop]
        );
    }

    #[test]
    fn test_vim_keys_map_to_movement_and_soft_drop() {
        let mut ih = InputHandler::new();

        assert_eq!(ih.handle_key_press(KeyCode::Char('h')), Some(GameAction::MoveLeft));
        ih.reset();
        assert_eq!(ih.handle_key_press(KeyCode::Char('l')), Some(GameAction::MoveRight));
        ih.reset();
        assert_eq!(ih.handle_key_press(KeyCode::Char('j')), Some(GameAction::SoftDrop));
    }

    #[test]
    fn test_reset_clears_held_state_and_stops_repeats() {
        let mut ih = InputHandler::with_config(100, 25).with_key_release_timeout_ms(10_000);

        assert_eq!(ih.handle_key_press(KeyCode::Left), Some(GameAction::MoveLeft));
        assert!(ih.update(200).len() > 0, "expected repeats before reset");

        ih.reset();
        assert!(ih.update(200).is_empty(), "reset should stop repeats");
    }
}
