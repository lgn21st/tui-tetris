//! Input handling for terminals that may not support key release events (like Ghostty)
//!
//! Design principles:
//! 1. Each key press generates exactly ONE immediate action
//! 2. DAS/ARR is simulated using timing - if a key is "held" for DAS time, ARR kicks in
//! 3. Key "release" is detected when a different key is pressed or after timeout
//! 4. This works on terminals with or without Release event support

use crossterm::event::KeyCode;

use crate::types::{GameAction, DEFAULT_ARR_MS, DEFAULT_DAS_MS};

/// Direction for horizontal movement
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalDirection {
    Left,
    Right,
    None,
}

/// Tracks input state for DAS/ARR handling
#[derive(Debug, Clone)]
pub struct InputHandler {
    /// Current horizontal direction (None, Left, or Right)
    horizontal: HorizontalDirection,

    /// Whether down key is currently "held"
    down_held: bool,

    /// Last key press timestamp (for detecting key release by timeout)
    last_key_time: std::time::Instant,

    /// DAS timer for horizontal movement
    horizontal_das_timer: u32,

    /// DAS timer for down movement
    down_das_timer: u32,

    /// ARR accumulator for horizontal
    horizontal_arr_accumulator: u32,

    /// ARR accumulator for down
    down_arr_accumulator: u32,

    /// DAS delay in milliseconds
    das_delay: u32,

    /// ARR rate in milliseconds
    arr_rate: u32,

    /// Key release timeout (if no new press in this time, consider key released)
    key_release_timeout_ms: u32,
}

impl InputHandler {
    /// Create a new input handler with default DAS/ARR settings
    pub fn new() -> Self {
        Self::with_config(DEFAULT_DAS_MS, DEFAULT_ARR_MS)
    }

    /// Create with custom DAS/ARR configuration
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
            key_release_timeout_ms: 150, // 150ms timeout for key "release"
        }
    }

    /// Handle key press event
    /// Returns Some(action) for immediate execution
    /// Also updates internal "held" state for DAS/ARR simulation
    pub fn handle_key_press(&mut self, code: KeyCode) -> Option<GameAction> {
        self.last_key_time = std::time::Instant::now();

        match code {
            // Left movement
            KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('A') => {
                let action = if self.horizontal == HorizontalDirection::Left {
                    // Same key pressed again - this is the "repeat" behavior
                    // Return None because DAS/ARR will handle continuous movement
                    None
                } else {
                    // First press or switched from another key
                    self.horizontal = HorizontalDirection::Left;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                    Some(GameAction::MoveLeft)
                };
                action
            }

            // Right movement
            KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('D') => {
                let action = if self.horizontal == HorizontalDirection::Right {
                    None
                } else {
                    self.horizontal = HorizontalDirection::Right;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                    Some(GameAction::MoveRight)
                };
                action
            }

            // Down movement
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') => {
                let action = if self.down_held {
                    None
                } else {
                    self.down_held = true;
                    self.down_das_timer = 0;
                    self.down_arr_accumulator = 0;
                    Some(GameAction::SoftDrop)
                };
                action
            }

            _ => None,
        }
    }

    /// Handle key release event (for terminals that support it)
    /// For Ghostty and similar terminals, this may never be called
    pub fn handle_key_release(&mut self, code: KeyCode) {
        match code {
            KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('A') => {
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
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') => {
                self.down_held = false;
                self.down_das_timer = 0;
                self.down_arr_accumulator = 0;
            }
            _ => {}
        }
    }

    /// Update timers and generate auto-repeat actions
    /// Call this every game tick with elapsed milliseconds
    /// Also handles automatic key "release" detection
    pub fn update(&mut self, elapsed_ms: u32) -> Vec<GameAction> {
        let mut actions = Vec::new();

        // Check for key "release" via timeout (for terminals without Release events)
        let time_since_last_key = self.last_key_time.elapsed().as_millis() as u32;
        if time_since_last_key > self.key_release_timeout_ms {
            // Key has been "released" due to timeout
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

        // Handle horizontal movement (left or right)
        match self.horizontal {
            HorizontalDirection::Left | HorizontalDirection::Right => {
                // Accumulate DAS time
                let prev_das = self.horizontal_das_timer;
                self.horizontal_das_timer += elapsed_ms;

                // Only start ARR after DAS delay
                if self.horizontal_das_timer >= self.das_delay {
                    // Calculate time available for ARR
                    let excess_time = if prev_das < self.das_delay {
                        self.horizontal_das_timer - self.das_delay
                    } else {
                        elapsed_ms
                    };
                    self.horizontal_arr_accumulator += excess_time;

                    // Generate repeat actions based on ARR rate
                    while self.horizontal_arr_accumulator >= self.arr_rate {
                        match self.horizontal {
                            HorizontalDirection::Left => actions.push(GameAction::MoveLeft),
                            HorizontalDirection::Right => actions.push(GameAction::MoveRight),
                            HorizontalDirection::None => {}
                        }
                        self.horizontal_arr_accumulator -= self.arr_rate;
                    }
                }
            }
            HorizontalDirection::None => {
                // Reset timers when no horizontal input
                self.horizontal_das_timer = 0;
                self.horizontal_arr_accumulator = 0;
            }
        }

        // Handle down movement
        if self.down_held {
            let prev_das = self.down_das_timer;
            self.down_das_timer += elapsed_ms;

            if self.down_das_timer >= self.das_delay {
                let excess_time = if prev_das < self.das_delay {
                    self.down_das_timer - self.das_delay
                } else {
                    elapsed_ms
                };
                self.down_arr_accumulator += excess_time;

                while self.down_arr_accumulator >= self.arr_rate {
                    actions.push(GameAction::SoftDrop);
                    self.down_arr_accumulator -= self.arr_rate;
                }
            }
        } else {
            self.down_das_timer = 0;
            self.down_arr_accumulator = 0;
        }

        actions
    }

    /// Reset all state (e.g., on game over or pause)
    pub fn reset(&mut self) {
        self.horizontal = HorizontalDirection::None;
        self.down_held = false;
        self.last_key_time = std::time::Instant::now();
        self.horizontal_das_timer = 0;
        self.down_das_timer = 0;
        self.horizontal_arr_accumulator = 0;
        self.down_arr_accumulator = 0;
    }

    /// Get current DAS delay
    pub fn das_delay(&self) -> u32 {
        self.das_delay
    }

    /// Get current ARR rate
    pub fn arr_rate(&self) -> u32 {
        self.arr_rate
    }

    /// Set DAS delay (for configuration)
    pub fn set_das_delay(&mut self, delay: u32) {
        self.das_delay = delay;
    }

    /// Set ARR rate (for configuration)
    pub fn set_arr_rate(&mut self, rate: u32) {
        self.arr_rate = rate;
    }

    /// Get current horizontal direction
    pub fn horizontal_direction(&self) -> HorizontalDirection {
        self.horizontal
    }

    /// Check if down is held
    pub fn is_down_held(&self) -> bool {
        self.down_held
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
    use crossterm::event::KeyCode;

    // === Basic Key Press Tests ===

    #[test]
    fn test_initial_state() {
        let handler = InputHandler::new();
        assert_eq!(handler.horizontal_direction(), HorizontalDirection::None);
        assert!(!handler.is_down_held());
    }

    #[test]
    fn test_left_key_press() {
        let mut handler = InputHandler::new();
        assert_eq!(
            handler.handle_key_press(KeyCode::Left),
            Some(GameAction::MoveLeft)
        );
        assert_eq!(handler.horizontal_direction(), HorizontalDirection::Left);
    }

    #[test]
    fn test_right_key_press() {
        let mut handler = InputHandler::new();
        assert_eq!(
            handler.handle_key_press(KeyCode::Right),
            Some(GameAction::MoveRight)
        );
        assert_eq!(handler.horizontal_direction(), HorizontalDirection::Right);
    }

    #[test]
    fn test_down_key_press() {
        let mut handler = InputHandler::new();
        assert_eq!(
            handler.handle_key_press(KeyCode::Down),
            Some(GameAction::SoftDrop)
        );
        assert!(handler.is_down_held());
    }

    #[test]
    fn test_wasd_keys() {
        let mut handler = InputHandler::new();
        assert_eq!(
            handler.handle_key_press(KeyCode::Char('a')),
            Some(GameAction::MoveLeft)
        );
        assert_eq!(
            handler.handle_key_press(KeyCode::Char('d')),
            Some(GameAction::MoveRight)
        );
        assert_eq!(
            handler.handle_key_press(KeyCode::Char('s')),
            Some(GameAction::SoftDrop)
        );
    }

    // === Key Release via Timeout Tests ===

    #[test]
    fn test_key_release_by_timeout() {
        let mut handler = InputHandler::with_config(50, 50);

        // Press left
        handler.handle_key_press(KeyCode::Left);
        assert_eq!(handler.horizontal_direction(), HorizontalDirection::Left);

        // Wait for timeout (simulate by not pressing any key)
        // Set last_key_time to past
        handler.last_key_time = std::time::Instant::now() - std::time::Duration::from_millis(200);

        // Update should detect "release" and stop horizontal movement
        let actions = handler.update(16);
        assert_eq!(handler.horizontal_direction(), HorizontalDirection::None);
        // Should not generate ARR actions after "release"
        assert!(actions.is_empty());
    }

    // === DAS/ARR Tests ===

    #[test]
    fn test_das_not_triggered_before_delay() {
        let mut handler = InputHandler::with_config(167, 33);

        handler.handle_key_press(KeyCode::Left);

        // Update before DAS triggers (100ms < 167ms)
        let actions = handler.update(100);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_das_triggers_arr() {
        let mut handler = InputHandler::with_config(100, 50);

        handler.handle_key_press(KeyCode::Left);

        // Update past DAS threshold (200ms >= 100ms DAS)
        // Reset last_key_time to prevent timeout
        handler.last_key_time = std::time::Instant::now();
        let actions = handler.update(200);

        // DAS triggers at 100ms, remaining 100ms = 2 ARR cycles
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&GameAction::MoveLeft));
    }

    #[test]
    fn test_arr_repeat_rate() {
        let mut handler = InputHandler::with_config(50, 50);

        handler.handle_key_press(KeyCode::Left);
        handler.last_key_time = std::time::Instant::now();

        // 150ms: 50ms DAS + 100ms ARR = 2 repeats
        let actions = handler.update(150);
        assert_eq!(actions.len(), 2);
        assert!(actions.iter().all(|&a| a == GameAction::MoveLeft));
    }

    #[test]
    fn test_das_reset_on_key_release() {
        let mut handler = InputHandler::with_config(100, 50);

        handler.handle_key_press(KeyCode::Left);
        handler.update(50); // Partial DAS

        handler.handle_key_release(KeyCode::Left);
        handler.handle_key_press(KeyCode::Left); // Press again

        // Should have full DAS again
        let actions = handler.update(60);
        assert!(actions.is_empty()); // 60ms < 100ms DAS
    }

    #[test]
    fn test_down_das_arr() {
        let mut handler = InputHandler::with_config(100, 50);

        handler.handle_key_press(KeyCode::Down);
        handler.last_key_time = std::time::Instant::now();

        let actions = handler.update(200);
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&GameAction::SoftDrop));
    }

    // === Reset Tests ===

    #[test]
    fn test_reset_clears_all_state() {
        let mut handler = InputHandler::new();

        handler.handle_key_press(KeyCode::Left);
        handler.handle_key_press(KeyCode::Down);
        handler.update(100);

        handler.reset();

        assert_eq!(handler.horizontal_direction(), HorizontalDirection::None);
        assert!(!handler.is_down_held());
    }

    // === Configuration Tests ===

    #[test]
    fn test_default_config() {
        let handler = InputHandler::new();
        assert_eq!(handler.das_delay(), 167);
        assert_eq!(handler.arr_rate(), 33);
    }

    #[test]
    fn test_custom_config() {
        let mut handler = InputHandler::with_config(100, 20);
        assert_eq!(handler.das_delay(), 100);
        assert_eq!(handler.arr_rate(), 20);

        handler.set_das_delay(150);
        handler.set_arr_rate(25);
        assert_eq!(handler.das_delay(), 150);
        assert_eq!(handler.arr_rate(), 25);
    }

    // === Edge Case Tests ===

    #[test]
    fn test_quick_alternation_no_jitter() {
        let mut handler = InputHandler::with_config(50, 50);

        // Rapid left-right-left-right alternation
        let action1 = handler.handle_key_press(KeyCode::Left);
        let action2 = handler.handle_key_press(KeyCode::Right);
        let action3 = handler.handle_key_press(KeyCode::Left);
        let action4 = handler.handle_key_press(KeyCode::Right);

        // Each press generates exactly one action
        assert_eq!(action1, Some(GameAction::MoveLeft));
        assert_eq!(action2, Some(GameAction::MoveRight));
        assert_eq!(action3, Some(GameAction::MoveLeft));
        assert_eq!(action4, Some(GameAction::MoveRight));

        // ARR should only generate actions for the last direction
        handler.last_key_time = std::time::Instant::now();
        let actions = handler.update(200);
        assert!(!actions.is_empty());
        // All ARR actions should be in the last pressed direction
        assert!(actions.iter().all(|&a| a == GameAction::MoveRight));
    }

    #[test]
    fn test_no_actions_when_no_key_held() {
        let mut handler = InputHandler::new();

        // No keys pressed
        let actions = handler.update(1000);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_unhandled_keys_return_none() {
        let mut handler = InputHandler::new();

        assert_eq!(handler.handle_key_press(KeyCode::Up), None);
        assert_eq!(handler.handle_key_press(KeyCode::Char(' ')), None);
        assert_eq!(handler.handle_key_press(KeyCode::Esc), None);
    }

    #[test]
    fn test_same_key_again_no_duplicate() {
        let mut handler = InputHandler::new();

        // First press
        let action1 = handler.handle_key_press(KeyCode::Left);
        assert_eq!(action1, Some(GameAction::MoveLeft));

        // Same key pressed again while "held" - no duplicate action
        // This simulates pressing the key rapidly
        let action2 = handler.handle_key_press(KeyCode::Left);
        assert_eq!(action2, None);
    }
}
