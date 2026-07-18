//! Deterministic application-level game session.
//!
//! This module is the single transition boundary shared by interactive,
//! headless, adapter, and test runners. Platform code collects commands; the
//! session applies them in a stable order and advances the core exactly once.

use arrayvec::ArrayVec;

use crate::core::{GameSnapshot, GameState};
use crate::engine::place::{apply_place, PlaceError};
use crate::types::{CoreLastEvent, GameAction, Rotation, TICK_MS};

pub const MAX_COMMANDS_PER_STEP: usize = 32;
pub const MAX_LOCAL_ACTIONS_PER_STEP: usize = 64;
pub const MAX_EVENTS_PER_STEP: usize = 4;

/// A platform-neutral command accepted at a fixed-step boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameCommand {
    Actions {
        actions: ArrayVec<GameAction, 32>,
        restart_seed: Option<u32>,
    },
    Place {
        x: i8,
        rotation: Rotation,
        use_hold: bool,
    },
}

impl GameCommand {
    pub fn action(action: GameAction) -> Self {
        let mut actions = ArrayVec::new();
        actions.push(action);
        Self::Actions {
            actions,
            restart_seed: None,
        }
    }
}

pub type CommandOutcome = Result<(), PlaceError>;

/// Complete input accepted at one authoritative logical-step boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepInput {
    pub remote: ArrayVec<GameCommand, MAX_COMMANDS_PER_STEP>,
    pub local: ArrayVec<GameAction, MAX_LOCAL_ACTIONS_PER_STEP>,
}

impl StepInput {
    pub fn with_remote(mut self, command: GameCommand) -> Self {
        self.remote.push(command);
        self
    }

    pub fn with_local(mut self, action: GameAction) -> Self {
        self.local.push(action);
        self
    }
}

/// Observable result of one authoritative logical transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub command_outcomes: ArrayVec<CommandOutcome, MAX_COMMANDS_PER_STEP>,
    pub events: ArrayVec<CoreLastEvent, MAX_EVENTS_PER_STEP>,
    pub changed: bool,
}

/// Coherent reusable projection of core state.
#[derive(Debug, Clone)]
pub struct SnapshotStore {
    snapshot: GameSnapshot,
    board_id: Option<u32>,
}

impl SnapshotStore {
    pub fn new(game: &GameState) -> Self {
        let mut store = Self {
            snapshot: GameSnapshot::default(),
            board_id: None,
        };
        store.refresh(game);
        store
    }

    pub fn refresh(&mut self, game: &GameState) -> &GameSnapshot {
        if self.board_id != Some(game.board_id()) {
            game.snapshot_board_into(&mut self.snapshot);
            self.board_id = Some(game.board_id());
        }
        game.snapshot_meta_into(&mut self.snapshot);
        &self.snapshot
    }

    pub fn get(&self) -> &GameSnapshot {
        &self.snapshot
    }
}

/// Owns the authoritative game and its coherent snapshot cache.
#[derive(Debug, Clone)]
pub struct SessionRuntime {
    game: GameState,
    snapshots: SnapshotStore,
    logical_step: u64,
}

impl SessionRuntime {
    pub fn new(seed: u32) -> Self {
        let mut game = GameState::new(seed);
        game.start();
        let snapshots = SnapshotStore::new(&game);
        Self {
            game,
            snapshots,
            logical_step: 0,
        }
    }

    /// Restores a session around an already constructed deterministic state.
    pub fn from_game(game: GameState) -> Self {
        let snapshots = SnapshotStore::new(&game);
        Self {
            game,
            snapshots,
            logical_step: 0,
        }
    }

    pub fn game(&self) -> &GameState {
        &self.game
    }

    pub fn snapshot(&self) -> &GameSnapshot {
        self.snapshots.get()
    }

    pub fn logical_step(&self) -> u64 {
        self.logical_step
    }

    /// Apply one complete input and advance exactly one logical transition.
    pub fn transition(&mut self, input: &StepInput) -> Transition {
        let before_snapshot = *self.snapshots.get();
        let mut command_outcomes = ArrayVec::new();

        for command in &input.remote {
            command_outcomes.push(apply_game_command(&mut self.game, command));
        }

        for &action in &input.local {
            let _ = self.game.apply_action(action);
        }

        let _ = self.game.tick(TICK_MS, false);
        let mut events = ArrayVec::new();
        if let Some(event) = self.game.take_last_event() {
            events.push(event);
        }
        self.logical_step = self.logical_step.wrapping_add(1);
        self.snapshots.refresh(&self.game);
        let changed = *self.snapshots.get() != before_snapshot || !events.is_empty();

        Transition {
            command_outcomes,
            events,
            changed,
        }
    }
}

fn apply_game_command(game: &mut GameState, command: &GameCommand) -> CommandOutcome {
    match command {
        GameCommand::Actions {
            actions,
            restart_seed,
        } => {
            let mut restart_seed = *restart_seed;
            for &action in actions {
                if action == GameAction::Restart {
                    if let Some(seed) = restart_seed.take() {
                        let _ = game.restart_with_seed(seed);
                        continue;
                    }
                }
                let _ = game.apply_action(action);
            }
            Ok(())
        }
        GameCommand::Place {
            x,
            rotation,
            use_hold,
        } => apply_place(game, *x, *rotation, *use_hold),
    }
}
