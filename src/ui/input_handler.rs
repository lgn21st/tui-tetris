//! Input handling with DAS/ARR (Delayed Auto Shift / Auto Repeat Rate)
//!
//! DAS: Time before auto-repeat starts when holding a key (default: 167ms)
//! ARR: Rate of auto-repeat after DAS triggers (default: 33ms)

use crossterm::event::KeyCode;

use crate::types::{GameAction, DEFAULT_ARR_MS, DEFAULT_DAS_MS};

/// Tracks input state for DAS/ARR handling
#[derive(Debug, Clone)]
pub struct InputHandler {
    // Key hold states
    left_held: bool,
    right_held: bool,
    down_held: bool,

    // DAS timers (time held, triggers at DEFAULT_DAS_MS)
    left_das_timer: u32,
    right_das_timer: u32,
    down_das_timer: u32,

    // ARR timers (accumulator for repeat rate)
    left_arr_accumulator: u32,
    right_arr_accumulator: u32,
    down_arr_accumulator: u32,

    // DAS/ARR configuration
    das_delay: u32,
    arr_rate: u32,
}

impl InputHandler {
    /// Create a new input handler with default DAS/ARR settings
    pub fn new() -> Self {
        Self::with_config(DEFAULT_DAS_MS, DEFAULT_ARR_MS)
    }

    /// Create with custom DAS/ARR configuration
    pub fn with_config(das_delay: u32, arr_rate: u32) -> Self {
        Self {
            left_held: false,
            right_held: false,
            down_held: false,
            left_das_timer: 0,
            right_das_timer: 0,
            down_das_timer: 0,
            left_arr_accumulator: 0,
            right_arr_accumulator: 0,
            down_arr_accumulator: 0,
            das_delay,
            arr_rate,
        }
    }

    /// Handle key press event
    pub fn handle_key_press(&mut self, code: KeyCode) -> Option<GameAction> {
        match code {
            KeyCode::Left => {
                self.left_held = true;
                self.left_das_timer = 0;
                self.left_arr_accumulator = 0;
                Some(GameAction::MoveLeft)
            }
            KeyCode::Right => {
                self.right_held = true;
                self.right_das_timer = 0;
                self.right_arr_accumulator = 0;
                Some(GameAction::MoveRight)
            }
            KeyCode::Down => {
                self.down_held = true;
                self.down_das_timer = 0;
                self.down_arr_accumulator = 0;
                Some(GameAction::SoftDrop)
            }
            _ => None,
        }
    }

    /// Handle key release event
    pub fn handle_key_release(&mut self, code: KeyCode) {
        match code {
            KeyCode::Left => {
                self.left_held = false;
                self.left_das_timer = 0;
                self.left_arr_accumulator = 0;
            }
            KeyCode::Right => {
                self.right_held = false;
                self.right_das_timer = 0;
                self.right_arr_accumulator = 0;
            }
            KeyCode::Down => {
                self.down_held = false;
                self.down_das_timer = 0;
                self.down_arr_accumulator = 0;
            }
            _ => {}
        }
    }

    /// Update timers and generate auto-repeat actions
    /// Call this every game tick with elapsed milliseconds
    pub fn update(&mut self, elapsed_ms: u32) -> Vec<GameAction> {
        let mut actions = Vec::new();

        // Handle left direction
        if self.left_held {
            let prev_das = self.left_das_timer;
            self.left_das_timer += elapsed_ms;

            // Check if DAS has triggered (either newly or already triggered)
            if self.left_das_timer >= self.das_delay {
                // Only add time that exceeds DAS delay to ARR accumulator
                if prev_das < self.das_delay {
                    // DAS just triggered this frame - add only the overflow time
                    self.left_arr_accumulator += self.left_das_timer - self.das_delay;
                } else {
                    // DAS already triggered - add full elapsed time
                    self.left_arr_accumulator += elapsed_ms;
                }

                // Generate actions based on ARR rate
                while self.left_arr_accumulator >= self.arr_rate {
                    actions.push(GameAction::MoveLeft);
                    self.left_arr_accumulator -= self.arr_rate;
                }
            }
        }

        // Handle right direction
        if self.right_held {
            let prev_das = self.right_das_timer;
            self.right_das_timer += elapsed_ms;

            if self.right_das_timer >= self.das_delay {
                if prev_das < self.das_delay {
                    self.right_arr_accumulator += self.right_das_timer - self.das_delay;
                } else {
                    self.right_arr_accumulator += elapsed_ms;
                }

                while self.right_arr_accumulator >= self.arr_rate {
                    actions.push(GameAction::MoveRight);
                    self.right_arr_accumulator -= self.arr_rate;
                }
            }
        }

        // Handle down direction (soft drop)
        if self.down_held {
            let prev_das = self.down_das_timer;
            self.down_das_timer += elapsed_ms;

            if self.down_das_timer >= self.das_delay {
                if prev_das < self.das_delay {
                    self.down_arr_accumulator += self.down_das_timer - self.das_delay;
                } else {
                    self.down_arr_accumulator += elapsed_ms;
                }

                while self.down_arr_accumulator >= self.arr_rate {
                    actions.push(GameAction::SoftDrop);
                    self.down_arr_accumulator -= self.arr_rate;
                }
            }
        }

        actions
    }

    /// Reset all state (e.g., on game over or pause)
    pub fn reset(&mut self) {
        self.left_held = false;
        self.right_held = false;
        self.down_held = false;
        self.left_das_timer = 0;
        self.right_das_timer = 0;
        self.down_das_timer = 0;
        self.left_arr_accumulator = 0;
        self.right_arr_accumulator = 0;
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

    #[test]
    fn test_key_press_generates_action() {
        let mut handler = InputHandler::new();

        // First press should generate action immediately
        assert_eq!(
            handler.handle_key_press(KeyCode::Left),
            Some(GameAction::MoveLeft)
        );
        assert_eq!(
            handler.handle_key_press(KeyCode::Right),
            Some(GameAction::MoveRight)
        );
        assert_eq!(
            handler.handle_key_press(KeyCode::Down),
            Some(GameAction::SoftDrop)
        );
    }

    #[test]
    fn test_key_release_clears_state() {
        let mut handler = InputHandler::new();

        // Press and hold
        handler.handle_key_press(KeyCode::Left);
        assert!(handler.left_held);

        // Release
        handler.handle_key_release(KeyCode::Left);
        assert!(!handler.left_held);
        assert_eq!(handler.left_das_timer, 0);
    }

    #[test]
    fn test_das_delay_no_repeat() {
        let mut handler = InputHandler::with_config(167, 33);

        // Press left
        handler.handle_key_press(KeyCode::Left);

        // Update before DAS triggers (166ms < 167ms)
        let actions = handler.update(166);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_das_triggers_arr() {
        let mut handler = InputHandler::with_config(100, 50);

        // Press left
        handler.handle_key_press(KeyCode::Left);

        // Update past DAS threshold
        let actions = handler.update(150);

        // Should have at least one repeat action
        // DAS triggers at 100ms, remaining 50ms generates one ARR
        assert!(!actions.is_empty());
        assert_eq!(actions[0], GameAction::MoveLeft);
    }

    #[test]
    fn test_arr_repeat_rate() {
        let mut handler = InputHandler::with_config(50, 50);

        // Press left
        handler.handle_key_press(KeyCode::Left);

        // First update triggers DAS + 2 ARR cycles
        let actions = handler.update(150); // 50ms DAS + 100ms = 2 ARR

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], GameAction::MoveLeft);
        assert_eq!(actions[1], GameAction::MoveLeft);
    }

    #[test]
    fn test_multiple_directions() {
        let mut handler = InputHandler::with_config(50, 50);

        // Hold both left and right
        handler.handle_key_press(KeyCode::Left);
        handler.handle_key_press(KeyCode::Right);

        // Update with enough time for DAS + 1 ARR each
        let actions = handler.update(100);

        // Should have actions from both directions
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&GameAction::MoveLeft));
        assert!(actions.contains(&GameAction::MoveRight));
    }

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

        // Test setters
        handler.set_das_delay(150);
        handler.set_arr_rate(25);
        assert_eq!(handler.das_delay(), 150);
        assert_eq!(handler.arr_rate(), 25);
    }

    #[test]
    fn test_reset() {
        let mut handler = InputHandler::new();

        handler.handle_key_press(KeyCode::Left);
        handler.update(100);

        handler.reset();

        assert!(!handler.left_held);
        assert_eq!(handler.left_das_timer, 0);
        assert_eq!(handler.left_arr_accumulator, 0);
    }
}
