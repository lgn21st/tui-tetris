//! Replay recording, verification, and inspection command surface.

use std::path::PathBuf;

use tetris_session::engine::replay::{ReplayTape, replay_and_verify};
use tetris_session::engine::session::StepInput;

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
}

pub fn parse_replay_args(args: &[String]) -> Result<Option<ReplayCommand>, String> {
    if args.first().map(String::as_str) != Some("replay") {
        return Ok(None);
    }
    let operation = args
        .get(1)
        .map(String::as_str)
        .ok_or("usage: tui-tetris replay <record|verify|inspect> <path> [--seed N] [--steps N]")?;
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
    }
}

fn read_tape(path: &PathBuf) -> Result<ReplayTape, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    ReplayTape::decode(&bytes)
}
