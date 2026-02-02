//! DAS/ARR input handler for terminal environments.
//!
//! Supports terminals that do not emit key release events by using a timeout.

use crossterm::event::KeyCode;

use crate::types::{GameAction, DEFAULT_ARR_MS, DEFAULT_DAS_MS};

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
}

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
            key_release_timeout_ms: 150,
        }
    }

    pub fn handle_key_press(&mut self, code: KeyCode) -> Option<GameAction> {
        self.last_key_time = std::time::Instant::now();

        match code {
            KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('A') => {
                if self.horizontal == HorizontalDirection::Left {
                    None
                } else {
                    self.horizontal = HorizontalDirection::Left;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                    Some(GameAction::MoveLeft)
                }
            }
            KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('D') => {
                if self.horizontal == HorizontalDirection::Right {
                    None
                } else {
                    self.horizontal = HorizontalDirection::Right;
                    self.horizontal_das_timer = 0;
                    self.horizontal_arr_accumulator = 0;
                    Some(GameAction::MoveRight)
                }
            }
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') => {
                if self.down_held {
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

    pub fn update(&mut self, elapsed_ms: u32) -> Vec<GameAction> {
        let mut actions = Vec::new();

        // Auto-release when terminal does not emit release events.
        let time_since_last_key = self.last_key_time.elapsed().as_millis() as u32;
        if time_since_last_key > self.key_release_timeout_ms {
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
                            HorizontalDirection::Left => actions.push(GameAction::MoveLeft),
                            HorizontalDirection::Right => actions.push(GameAction::MoveRight),
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

            if self.down_das_timer >= self.das_delay {
                let excess = if prev_das < self.das_delay {
                    self.down_das_timer - self.das_delay
                } else {
                    elapsed_ms
                };
                self.down_arr_accumulator += excess;
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

    pub fn reset(&mut self) {
        self.horizontal = HorizontalDirection::None;
        self.down_held = false;
        self.last_key_time = std::time::Instant::now();
        self.horizontal_das_timer = 0;
        self.down_das_timer = 0;
        self.horizontal_arr_accumulator = 0;
        self.down_arr_accumulator = 0;
    }
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}
