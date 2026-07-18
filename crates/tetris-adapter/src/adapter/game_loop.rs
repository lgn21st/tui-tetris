//! Synchronous adapter work performed at the fixed-step game-loop boundary.

use std::sync::Arc;

use arrayvec::ArrayVec;

use crate::adapter::command_apply::map_place_error_code;
use crate::adapter::observation_schedule::ObservationSchedule;
use crate::adapter::protocol::{StateHash, create_applied_ack, create_error};
use crate::adapter::runtime::{Adapter, InboundPayload, OutboundMessage};
use crate::adapter::server::build_observation;
use tetris_core::types::GameAction;
use tetris_session::engine::replay::transition_hash;
use tetris_session::engine::session::{GameCommand, SessionRuntime, StepInput, Transition};

pub const MAX_COMMANDS_PER_STEP: usize = 32;

struct PendingCommand {
    seq: u64,
    command: GameCommand,
    responder: crate::adapter::runtime::ClientResponder,
}

/// Transport-independent protocol driver used by synchronous runners and tests.
///
/// It owns no socket or queue. An inbound message already authenticated by the
/// broker is applied through the production session and answered through its
/// originating client responder.
pub struct SessionProtocolDriver {
    session: SessionRuntime,
    observations: ObservationSchedule,
    post_command_steps: u8,
}

impl SessionProtocolDriver {
    pub fn new(seed: u32, observation_hz: u32) -> Self {
        let session = SessionRuntime::new(seed);
        Self::from_session(session, observation_hz)
    }

    pub fn from_session(session: SessionRuntime, observation_hz: u32) -> Self {
        let observations = ObservationSchedule::new(session.game(), observation_hz);
        Self {
            session,
            observations,
            post_command_steps: 0,
        }
    }

    /// Advance additional fixed steps before replying with the follow-up snapshot.
    /// Useful for non-realtime batch runners that otherwise have no outer clock.
    pub fn with_post_command_steps(mut self, steps: u8) -> Self {
        self.post_command_steps = steps;
        self
    }

    pub fn session(&self) -> &SessionRuntime {
        &self.session
    }

    pub fn handle(&mut self, inbound: crate::adapter::runtime::InboundCommand) {
        match inbound.payload {
            InboundPayload::SnapshotRequest => {
                let (seq, events) = self.observations.immediate();
                let observation = build_observation(
                    seq,
                    self.session.logical_step(),
                    self.session.snapshot(),
                    &events,
                );
                let _ = inbound.responder.send_observation(Arc::new(observation));
            }
            InboundPayload::Command(command) => {
                let input = StepInput::default().with_remote(command);
                let transition = self.session.transition(&input);
                match transition.command_outcomes.first() {
                    Some(Ok(())) => {
                        let state_hash =
                            transition_hash(self.session.snapshot(), 0, &transition.events, &[]);
                        let _ = inbound.responder.send_ack(create_applied_ack(
                            inbound.seq,
                            inbound.seq,
                            self.session.logical_step(),
                            StateHash(state_hash),
                        ));
                    }
                    Some(Err(error)) => {
                        let _ = inbound.responder.send_error(create_error(
                            inbound.seq,
                            map_place_error_code(*error),
                            error.message(),
                        ));
                    }
                    None => unreachable!("one command must produce one outcome"),
                }
                for &event in &transition.events {
                    self.observations.capture_event(event);
                }
                for _ in 0..self.post_command_steps {
                    let idle = self.session.transition(&StepInput::default());
                    for &event in &idle.events {
                        self.observations.capture_event(event);
                    }
                }
                let (seq, events) = self.observations.immediate();
                let observation = build_observation(
                    seq,
                    self.session.logical_step(),
                    self.session.snapshot(),
                    &events,
                );
                let _ = inbound.responder.send_observation(Arc::new(observation));
            }
        }
    }
}

/// Execute the single authoritative application step shared by every runner.
///
/// Snapshot requests observe the latest completed step. Gameplay commands are
/// applied before local actions, followed by exactly one core tick. Correlated
/// responses are emitted only after that application step has completed.
pub fn step_session(
    adapter: &mut Option<Adapter>,
    session: &mut SessionRuntime,
    observations: &mut ObservationSchedule,
    local_actions: &[GameAction],
    has_streaming_subscribers: bool,
) -> Transition {
    let mut pending = ArrayVec::<PendingCommand, MAX_COMMANDS_PER_STEP>::new();

    if let Some(adapter) = adapter.as_mut() {
        for _ in 0..MAX_COMMANDS_PER_STEP {
            let Some(inbound) = adapter.try_recv() else {
                break;
            };
            match inbound.payload {
                InboundPayload::SnapshotRequest => {
                    let (seq, events) = observations.immediate();
                    let observation =
                        build_observation(seq, session.logical_step(), session.snapshot(), &events);
                    let _ = inbound.responder.send_observation(Arc::new(observation));
                }
                InboundPayload::Command(command) => pending.push(PendingCommand {
                    seq: inbound.seq,
                    command,
                    responder: inbound.responder,
                }),
            }
        }
    }

    let mut input = StepInput::default();
    input
        .remote
        .extend(pending.iter().map(|item| item.command.clone()));
    input.local.extend(local_actions.iter().copied());
    let transition = session.transition(&input);

    for (pending, outcome) in pending.iter().zip(transition.command_outcomes.iter()) {
        match outcome {
            Ok(()) => {
                let state_hash = transition_hash(session.snapshot(), 0, &transition.events, &[]);
                let _ = pending.responder.send_ack(create_applied_ack(
                    pending.seq,
                    pending.seq,
                    session.logical_step(),
                    StateHash(state_hash),
                ));
            }
            Err(error) => {
                let _ = pending.responder.send_error(create_error(
                    pending.seq,
                    map_place_error_code(*error),
                    error.message(),
                ));
            }
        }
    }

    for &event in &transition.events {
        observations.capture_event(event);
    }
    if let Some((seq, events)) = observations.after_tick(session.game())
        && has_streaming_subscribers
        && let Some(adapter) = adapter.as_ref()
    {
        let observation =
            build_observation(seq, session.logical_step(), session.snapshot(), &events);
        let _ = adapter.send(OutboundMessage::BroadcastObservationArc {
            obs: Arc::new(observation),
        });
    }

    transition
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetris_core::types::GameAction;
    use tetris_session::engine::session::SessionRuntime;

    #[test]
    fn shared_step_advances_session_without_an_adapter() {
        let mut session = SessionRuntime::new(1);
        let mut observations = ObservationSchedule::new(session.game(), 20);
        let mut adapter = None;

        let result = step_session(
            &mut adapter,
            &mut session,
            &mut observations,
            &[GameAction::HardDrop],
            false,
        );

        assert!(result.events.first().expect("lock event").locked);
        assert_eq!(session.snapshot().board_id, session.game().board_id());
        assert_eq!(session.game().step_in_piece(), 1);
    }
}
