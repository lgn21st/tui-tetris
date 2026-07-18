//! Top-level non-interactive application commands.

use crate::adapter::protocol::PROTOCOL_VERSION;
use crate::engine::replay::{transition_hash, REPLAY_FORMAT_VERSION, RULESET_VERSION};
use crate::engine::session::{SessionRuntime, StepInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadlessConfig {
    pub seed: u32,
    pub steps: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommand {
    Headless(HeadlessConfig),
    Diagnostic,
}

pub fn parse_app_args(args: &[String]) -> Result<Option<AppCommand>, String> {
    match args.first().map(String::as_str) {
        Some("diagnostic") if args.len() == 1 => Ok(Some(AppCommand::Diagnostic)),
        Some("diagnostic") => Err("diagnostic takes no arguments".into()),
        Some("headless") => {
            let mut seed = 1;
            let mut steps = None;
            let mut index = 1;
            while index < args.len() {
                let value = args.get(index + 1).ok_or("missing headless option value")?;
                match args[index].as_str() {
                    "--seed" => seed = value.parse().map_err(|_| "invalid --seed")?,
                    "--steps" => steps = Some(value.parse().map_err(|_| "invalid --steps")?),
                    option => return Err(format!("unknown headless option: {option}")),
                }
                index += 2;
            }
            Ok(Some(AppCommand::Headless(HeadlessConfig { seed, steps })))
        }
        _ => Ok(None),
    }
}

pub fn run_batch_headless(config: HeadlessConfig) -> Result<String, String> {
    let steps = config
        .steps
        .ok_or("batch headless mode requires a finite --steps value")?;
    let mut session = SessionRuntime::new(config.seed);
    let input = StepInput::default();
    let mut hash = transition_hash(session.snapshot(), 0, &[], &[]);
    for _ in 0..steps {
        let transition = session.transition(&input);
        hash = transition_hash(
            session.snapshot(),
            session.logical_step(),
            &transition.events,
            &transition.command_outcomes,
        );
    }
    Ok(format!(
        "seed={} steps={} state_hash={hash:016x}",
        config.seed, steps
    ))
}

pub fn diagnostic_report() -> String {
    format!(
        "protocol={PROTOCOL_VERSION}\nreplay=TTR{REPLAY_FORMAT_VERSION}\nruleset={RULESET_VERSION}\narchitecture=core,session,adapter-protocol,adapter,terminal,app"
    )
}
