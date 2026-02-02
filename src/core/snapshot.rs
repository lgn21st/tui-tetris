use crate::core::Tetromino;
use crate::types::{PieceKind, Rotation, BOARD_HEIGHT, BOARD_WIDTH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActiveSnapshot {
    pub kind: PieceKind,
    pub rotation: Rotation,
    pub x: i8,
    pub y: i8,
}

impl From<Tetromino> for ActiveSnapshot {
    fn from(value: Tetromino) -> Self {
        Self {
            kind: value.kind,
            rotation: value.rotation,
            x: value.x,
            y: value.y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimersSnapshot {
    pub drop_ms: u32,
    pub lock_ms: u32,
    pub line_clear_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameSnapshot {
    pub board: [[u8; BOARD_WIDTH as usize]; BOARD_HEIGHT as usize],
    pub board_id: u32,
    pub active: Option<ActiveSnapshot>,
    pub ghost_y: Option<i8>,
    pub hold: Option<PieceKind>,
    pub next_queue: [PieceKind; 5],
    pub can_hold: bool,
    pub paused: bool,
    pub game_over: bool,
    pub episode_id: u32,
    pub seed: u32,
    pub piece_id: u32,
    pub step_in_piece: u32,
    pub score: u32,
    pub level: u32,
    pub lines: u32,
    pub timers: TimersSnapshot,
}

impl GameSnapshot {
    pub fn clear(&mut self) {
        self.board = [[0u8; BOARD_WIDTH as usize]; BOARD_HEIGHT as usize];
        self.board_id = 0;
        self.active = None;
        self.ghost_y = None;
        self.hold = None;
        self.next_queue = [PieceKind::I; 5];
        self.can_hold = true;
        self.paused = false;
        self.game_over = false;
        self.episode_id = 0;
        self.seed = 0;
        self.piece_id = 0;
        self.step_in_piece = 0;
        self.score = 0;
        self.level = 0;
        self.lines = 0;
        self.timers = TimersSnapshot {
            drop_ms: 0,
            lock_ms: 0,
            line_clear_ms: 0,
        };
    }

    pub fn playable(&self) -> bool {
        !self.game_over && !self.paused
    }
}

impl Default for GameSnapshot {
    fn default() -> Self {
        let mut s = Self {
            board: [[0u8; BOARD_WIDTH as usize]; BOARD_HEIGHT as usize],
            board_id: 0,
            active: None,
            ghost_y: None,
            hold: None,
            next_queue: [PieceKind::I; 5],
            can_hold: true,
            paused: false,
            game_over: false,
            episode_id: 0,
            seed: 0,
            piece_id: 0,
            step_in_piece: 0,
            score: 0,
            level: 0,
            lines: 0,
            timers: TimersSnapshot {
                drop_ms: 0,
                lock_ms: 0,
                line_clear_ms: 0,
            },
        };
        s.clear();
        s
    }
}
