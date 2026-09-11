//! Replay recording, verification, inspection, and budgeted evaluation.

use std::path::PathBuf;

use crate::app_cli::Budget;
use tetris_session::engine::replay::{ReplayTape, replay_and_verify, transition_hash};
use tetris_session::engine::session::{SessionRuntime, StepInput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayCommand {
    Record {
        path: PathBuf,
        seed: u32,
        steps: u64,
    },
    Verify {
        path: PathBuf,
    },
    Inspect {
        path: PathBuf,
    },
    /// Score a recorded run at a fixed line/piece budget.
    Eval {
        path: PathBuf,
        budget: Budget,
    },
}

pub fn parse_replay_args(args: &[String]) -> Result<Option<ReplayCommand>, String> {
    if args.first().map(String::as_str) != Some("replay") {
        return Ok(None);
    }
    let operation = args.get(1).map(String::as_str).ok_or(
        "usage: tui-tetris replay <record|verify|inspect|eval> <path> [--seed N] [--steps N] \
         [--budget-lines N] [--budget-pieces N]",
    )?;
    let path = args
        .get(2)
        .map(PathBuf::from)
        .ok_or("missing replay path")?;
    match operation {
        "record" => {
            let mut seed = 1;
            let mut steps = 0;
            let mut index = 3;
            while index < args.len() {
                let value = args.get(index + 1).ok_or("missing replay option value")?;
                match args[index].as_str() {
                    "--seed" => seed = value.parse().map_err(|_| "invalid --seed")?,
                    "--steps" => steps = value.parse().map_err(|_| "invalid --steps")?,
                    option => return Err(format!("unknown replay option: {option}")),
                }
                index += 2;
            }
            Ok(Some(ReplayCommand::Record { path, seed, steps }))
        }
        "eval" => {
            let mut budget = Budget::default();
            let mut index = 3;
            while index < args.len() {
                let value = args.get(index + 1).ok_or("missing replay option value")?;
                let option = args[index].as_str();
                if !budget.apply_flag(option, value)? {
                    return Err(format!("unknown replay option: {option}"));
                }
                index += 2;
            }
            Ok(Some(ReplayCommand::Eval { path, budget }))
        }
        "verify" if args.len() == 3 => Ok(Some(ReplayCommand::Verify { path })),
        "inspect" if args.len() == 3 => Ok(Some(ReplayCommand::Inspect { path })),
        "verify" | "inspect" => Err("unexpected replay arguments".into()),
        _ => Err(format!("unknown replay operation: {operation}")),
    }
}

pub fn run_replay_command(command: ReplayCommand) -> Result<String, String> {
    match command {
        ReplayCommand::Record { path, seed, steps } => {
            let tape = ReplayTape::record(seed, (0..steps).map(|_| StepInput::default()));
            std::fs::write(&path, tape.encode()).map_err(|error| error.to_string())?;
            Ok(format!("recorded {steps} steps to {}", path.display()))
        }
        ReplayCommand::Verify { path } => {
            let tape = read_tape(&path)?;
            replay_and_verify(&tape).map_err(|mismatch| {
                format!(
                    "replay mismatch at step {}: expected {}, actual {}",
                    mismatch.step, mismatch.expected, mismatch.actual
                )
            })?;
            Ok(format!(
                "verified {} steps from {}",
                tape.records().len(),
                path.display()
            ))
        }
        ReplayCommand::Inspect { path } => {
            let tape = read_tape(&path)?;
            Ok(format!(
                "path: {}\nruleset: {}\nseed: {}\nsteps: {}\nfinal_state_hash: {}",
                path.display(),
                tape.ruleset_version(),
                tape.seed(),
                tape.records().len(),
                tape.records().last().map_or(0, |record| record.state_hash)
            ))
        }
        ReplayCommand::Eval { path, budget } => eval_tape(&path, budget),
    }
}

/// Replay a tape, stop at its budget, and report the comparable score.
///
/// The tape already captures a policy's decisions, so this needs no built-in
/// policy. Step hashes are checked while replaying, but a mismatch is reported
/// rather than aborting the score, so a corrupted tape is visible instead of
/// silently scored.
fn eval_tape(path: &PathBuf, budget: Budget) -> Result<String, String> {
    let tape = read_tape(path)?;
    let mut session = SessionRuntime::new(tape.seed());
    let mut completed = 0usize;
    let mut budget_reached = false;
    let mut first_mismatch = None;

    for record in tape.records() {
        let transition = session.transition(&record.input);
        let actual = transition_hash(
            session.snapshot(),
            session.logical_step(),
            &transition.events,
            &transition.command_outcomes,
        );
        if actual != record.state_hash && first_mismatch.is_none() {
            first_mismatch = Some(record.step);
        }
        completed += 1;
        if budget.is_reached(session.game()) {
            budget_reached = true;
            break;
        }
        if session.game().game_over() {
            break;
        }
    }

    let game = session.game();
    let verdict = match first_mismatch {
        None => "ok".to_string(),
        Some(step) => format!("mismatch_at_step:{step}"),
    };
    Ok(format!(
        "path: {}\nruleset: {}\nseed: {}\nbudget: {}\nsteps: {}\npieces: {}\nlines: {}\nscore: {}\n\
         budget_reached: {}\ntopped_out: {}\nreplay_check: {verdict}",
        path.display(),
        tape.ruleset_version(),
        tape.seed(),
        budget.describe(),
        completed,
        game.piece_id(),
        game.lines(),
        game.score(),
        u8::from(budget_reached),
        u8::from(game.game_over()),
    ))
}

fn read_tape(path: &PathBuf) -> Result<ReplayTape, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    ReplayTape::decode(&bytes)
}
