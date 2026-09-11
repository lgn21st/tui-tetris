//! Top-level non-interactive application commands.

use tetris_adapter_protocol::protocol::PROTOCOL_VERSION;
use tetris_core::core::GameState;
use tetris_core::types::TIMING_PROFILE_ID;
use tetris_session::engine::replay::{
    EFFECTIVE_RULESET_ID, REPLAY_FORMAT_VERSION, RULESET_VERSION, transition_hash,
};
use tetris_session::engine::session::{SessionRuntime, StepInput};

/// Bounded evaluation budget: stop a run once either bound is met.
///
/// Score grows with level and gravity is capped at a normal-mode ceiling, so
/// unbounded survival is not a ranking metric — a policy that merely avoids
/// topping out converges on the same score as a strong one. Scoring at a fixed
/// line or piece budget (the shape of an official line-clear Marathon) makes
/// runs comparable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Budget {
    pub max_lines: Option<u32>,
    pub max_pieces: Option<u32>,
}

impl Budget {
    /// Parse a `--budget-lines` / `--budget-pieces` flag.
    ///
    /// Returns `false` when `flag` is not a budget option, so callers can keep
    /// their own unknown-option error.
    pub fn apply_flag(&mut self, flag: &str, value: &str) -> Result<bool, String> {
        match flag {
            "--budget-lines" => {
                self.max_lines = Some(value.parse().map_err(|_| "invalid --budget-lines")?);
                Ok(true)
            }
            "--budget-pieces" => {
                self.max_pieces = Some(value.parse().map_err(|_| "invalid --budget-pieces")?);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Whether the run has consumed its budget.
    pub fn is_reached(&self, game: &GameState) -> bool {
        self.max_lines.is_some_and(|max| game.lines() >= max)
            || self.max_pieces.is_some_and(|max| game.piece_id() >= max)
    }

    /// Machine-readable bound, e.g. `lines:150`, `lines:150,pieces:500`, `none`.
    pub fn describe(&self) -> String {
        match (self.max_lines, self.max_pieces) {
            (None, None) => "none".into(),
            (Some(lines), None) => format!("lines:{lines}"),
            (None, Some(pieces)) => format!("pieces:{pieces}"),
            (Some(lines), Some(pieces)) => format!("lines:{lines},pieces:{pieces}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadlessConfig {
    pub seed: u32,
    pub steps: Option<u64>,
    pub budget: Budget,
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
            let mut budget = Budget::default();
            let mut index = 1;
            while index < args.len() {
                let value = args.get(index + 1).ok_or("missing headless option value")?;
                match args[index].as_str() {
                    "--seed" => seed = value.parse().map_err(|_| "invalid --seed")?,
                    "--steps" => steps = Some(value.parse().map_err(|_| "invalid --steps")?),
                    option => {
                        if !budget.apply_flag(option, value)? {
                            return Err(format!("unknown headless option: {option}"));
                        }
                    }
                }
                index += 2;
            }
            Ok(Some(AppCommand::Headless(HeadlessConfig {
                seed,
                steps,
                budget,
            })))
        }
        _ => Ok(None),
    }
}

/// Run a finite headless batch and report the score at its budget.
///
/// The run stops at the requested step count, when the budget is consumed, or
/// on game over — whichever comes first — so the reported `steps` is what
/// actually executed.
pub fn run_batch_headless(config: HeadlessConfig) -> Result<String, String> {
    let steps = config
        .steps
        .ok_or("batch headless mode requires a finite --steps value")?;
    let mut session = SessionRuntime::new(config.seed);
    let input = StepInput::default();
    let mut hash = transition_hash(session.snapshot(), 0, &[], &[]);
    let mut completed = 0u64;
    let mut budget_reached = false;

    for _ in 0..steps {
        let transition = session.transition(&input);
        hash = transition_hash(
            session.snapshot(),
            session.logical_step(),
            &transition.events,
            &transition.command_outcomes,
        );
        completed += 1;
        if config.budget.is_reached(session.game()) {
            budget_reached = true;
            break;
        }
        if session.game().game_over() {
            break;
        }
    }

    let game = session.game();
    Ok(format!(
        "seed={} steps={completed} requested_steps={steps} budget={} budget_reached={} \
         pieces={} lines={} score={} topped_out={} state_hash={hash:016x}",
        config.seed,
        config.budget.describe(),
        u8::from(budget_reached),
        game.piece_id(),
        game.lines(),
        game.score(),
        u8::from(game.game_over()),
    ))
}

/// Report for a live run that stopped on its budget or on game over.
pub fn run_report(config: HeadlessConfig, session: &SessionRuntime) -> String {
    let game = session.game();
    format!(
        "seed={} steps={} budget={} budget_reached={} pieces={} lines={} score={} topped_out={}",
        config.seed,
        session.logical_step(),
        config.budget.describe(),
        u8::from(config.budget.is_reached(game)),
        game.piece_id(),
        game.lines(),
        game.score(),
        u8::from(game.game_over()),
    )
}

pub fn diagnostic_report() -> String {
    format!(
        "protocol={PROTOCOL_VERSION}\nreplay=TTR{REPLAY_FORMAT_VERSION}\nruleset={EFFECTIVE_RULESET_ID}\nportable_ruleset={RULESET_VERSION}\ntiming={TIMING_PROFILE_ID}\narchitecture=core,session,adapter-protocol,adapter,terminal,app"
    )
}
