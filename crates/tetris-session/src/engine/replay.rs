//! Stable command recording and deterministic replay verification.

use crate::core::{stable_state_hash, GameSnapshot};
use crate::engine::session::{CommandOutcome, GameCommand, SessionRuntime, StepInput};
use crate::types::{GameAction, Rotation};
use arrayvec::ArrayVec;

pub const REPLAY_FORMAT_VERSION: u16 = 2;
pub const RULESET_VERSION: &str = "tui-guideline-2026.1";

const HASH_PRIME: u64 = 0x100000001b3;

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(HASH_PRIME);
    }
}

/// Stable hash of the complete observable result of one logical transition.
pub fn transition_hash(
    snapshot: &GameSnapshot,
    logical_step: u64,
    events: &[crate::types::CoreLastEvent],
    outcomes: &[CommandOutcome],
) -> u64 {
    let mut hash = stable_state_hash(snapshot, None);
    hash_bytes(&mut hash, &logical_step.to_le_bytes());
    hash_bytes(&mut hash, &(events.len() as u64).to_le_bytes());
    for event in events {
        let event_hash = stable_state_hash(snapshot, Some(*event));
        hash_bytes(&mut hash, &event_hash.to_le_bytes());
    }
    hash_bytes(&mut hash, &(outcomes.len() as u64).to_le_bytes());
    for outcome in outcomes {
        let code = match outcome {
            Ok(()) => 0,
            Err(error) => match error {
                crate::engine::place::PlaceError::HoldUnavailable => 1,
                crate::engine::place::PlaceError::RotationBlocked => 2,
                crate::engine::place::PlaceError::XOutOfBounds => 3,
                crate::engine::place::PlaceError::XBlocked => 4,
                crate::engine::place::PlaceError::NotPlayable => 5,
                crate::engine::place::PlaceError::NoActive => 6,
            },
        };
        hash_bytes(&mut hash, &[code]);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    pub step: u64,
    pub input: StepInput,
    pub state_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayTape {
    seed: u32,
    records: Vec<StepRecord>,
    final_snapshot: GameSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayMismatch {
    pub step: usize,
    pub expected: u64,
    pub actual: u64,
}

impl ReplayTape {
    pub fn record(seed: u32, inputs: impl IntoIterator<Item = StepInput>) -> Self {
        let mut session = SessionRuntime::new(seed);
        let mut records = Vec::new();
        for (step, input) in inputs.into_iter().enumerate() {
            let transition = session.transition(&input);
            records.push(StepRecord {
                step: step as u64,
                state_hash: transition_hash(
                    session.snapshot(),
                    session.logical_step(),
                    &transition.events,
                    &transition.command_outcomes,
                ),
                input,
            });
        }
        Self {
            seed,
            records,
            final_snapshot: *session.snapshot(),
        }
    }

    pub fn records(&self) -> &[StepRecord] {
        &self.records
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    pub fn ruleset_version(&self) -> &'static str {
        RULESET_VERSION
    }

    pub fn final_snapshot(&self) -> &GameSnapshot {
        &self.final_snapshot
    }

    pub fn minimal_failure_prefix(&self, mismatch: &ReplayMismatch) -> Self {
        let records = self.records[..=mismatch.step].to_vec();
        let mut prefix = Self::record(self.seed, records.iter().map(|record| record.input.clone()));
        prefix.records = records;
        prefix
    }

    #[doc(hidden)]
    pub fn replace_record_for_test(&mut self, index: usize, record: StepRecord) {
        self.records[index] = record;
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut output = format!(
            "TTR{REPLAY_FORMAT_VERSION}\t{RULESET_VERSION}\t{}\n",
            self.seed
        );
        for record in &self.records {
            output.push_str(&format!("S\t{}\t{}\n", record.step, record.state_hash));
            for command in &record.input.remote {
                match command {
                    GameCommand::Actions {
                        actions,
                        restart_seed,
                    } => {
                        let seed = restart_seed.map_or_else(|| "-".into(), |v| v.to_string());
                        let actions = actions
                            .iter()
                            .map(GameAction::as_str)
                            .collect::<Vec<_>>()
                            .join(",");
                        output.push_str(&format!("R\tA\t{seed}\t{actions}\n"));
                    }
                    GameCommand::Place {
                        x,
                        rotation,
                        use_hold,
                    } => output.push_str(&format!(
                        "R\tP\t{x}\t{}\t{}\n",
                        rotation.as_str(),
                        use_hold
                    )),
                }
            }
            for action in &record.input.local {
                output.push_str(&format!("L\t{}\n", action.as_str()));
            }
            output.push_str("E\n");
        }
        output.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let mut lines = text.lines();
        let header = lines.next().ok_or("missing replay header")?;
        let mut header = header.split('\t');
        let format = header.next().ok_or("missing replay format")?;
        if format != format!("TTR{REPLAY_FORMAT_VERSION}") {
            return Err(format!("unsupported replay format: {format}"));
        }
        let ruleset = header.next().ok_or("missing replay ruleset")?;
        if ruleset != RULESET_VERSION {
            return Err(format!("unsupported ruleset: {ruleset}"));
        }
        let seed = header
            .next()
            .ok_or("missing replay seed")?
            .parse::<u32>()
            .map_err(|error| error.to_string())?;
        if header.next().is_some() {
            return Err("invalid replay header".into());
        }
        let mut records = Vec::new();
        while let Some(line) = lines.next() {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 || fields[0] != "S" {
                return Err("invalid step record".into());
            }
            let step = fields[1]
                .parse::<u64>()
                .map_err(|error| error.to_string())?;
            let state_hash = fields[2]
                .parse::<u64>()
                .map_err(|error| error.to_string())?;
            let mut input = StepInput::default();
            loop {
                let line = lines.next().ok_or("unterminated step")?;
                if line == "E" {
                    break;
                }
                let fields = line.split('\t').collect::<Vec<_>>();
                match fields.as_slice() {
                    ["L", action] => input
                        .local
                        .try_push(action.parse().map_err(|_| "invalid local action")?)
                        .map_err(|_| "too many local actions")?,
                    ["R", "A", seed, actions] => {
                        let restart_seed = if *seed == "-" {
                            None
                        } else {
                            Some(seed.parse::<u32>().map_err(|error| error.to_string())?)
                        };
                        let mut parsed = ArrayVec::new();
                        if !actions.is_empty() {
                            for action in actions.split(',') {
                                parsed
                                    .try_push(action.parse().map_err(|_| "invalid remote action")?)
                                    .map_err(|_| "too many remote actions")?;
                            }
                        }
                        input
                            .remote
                            .try_push(GameCommand::Actions {
                                actions: parsed,
                                restart_seed,
                            })
                            .map_err(|_| "too many commands")?;
                    }
                    ["R", "P", x, rotation, use_hold] => input
                        .remote
                        .try_push(GameCommand::Place {
                            x: x.parse::<i8>().map_err(|error| error.to_string())?,
                            rotation: rotation
                                .parse::<Rotation>()
                                .map_err(|_| "invalid rotation")?,
                            use_hold: use_hold
                                .parse::<bool>()
                                .map_err(|error| error.to_string())?,
                        })
                        .map_err(|_| "too many commands")?,
                    _ => return Err("invalid command record".into()),
                }
            }
            records.push(StepRecord {
                step,
                input,
                state_hash,
            });
        }
        let mut tape = Self::record(seed, records.iter().map(|record| record.input.clone()));
        tape.records = records;
        Ok(tape)
    }
}

pub fn replay_and_verify(tape: &ReplayTape) -> Result<SessionRuntime, ReplayMismatch> {
    let mut session = SessionRuntime::new(tape.seed);
    for (index, record) in tape.records.iter().enumerate() {
        let transition = session.transition(&record.input);
        let actual = transition_hash(
            session.snapshot(),
            session.logical_step(),
            &transition.events,
            &transition.command_outcomes,
        );
        if actual != record.state_hash {
            return Err(ReplayMismatch {
                step: index,
                expected: record.state_hash,
                actual,
            });
        }
    }
    Ok(session)
}
