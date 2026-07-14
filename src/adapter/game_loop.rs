//! Synchronous adapter work performed at the fixed-step game-loop boundary.

use std::sync::Arc;

use crate::adapter::command_apply::{apply_client_command, map_place_error_code};
use crate::adapter::observation_schedule::ObservationSchedule;
use crate::adapter::protocol::{create_ack, create_error};
use crate::adapter::runtime::{Adapter, InboundPayload, OutboundMessage};
use crate::adapter::server::build_observation;
use crate::core::{GameSnapshot, GameState};

pub const MAX_COMMANDS_PER_STEP: usize = 32;

/// Drain a bounded number of commands before the current logic tick.
///
/// Keeping this operation in one place preserves identical command ordering and
/// acknowledgement behavior in interactive and headless runners.
pub fn drain_commands(
    adapter: &mut Option<Adapter>,
    game_state: &mut GameState,
    observations: &mut ObservationSchedule,
    snapshot: &mut GameSnapshot,
    last_board_id: &mut u32,
) {
    let Some(adapter) = adapter.as_mut() else {
        return;
    };

    for _ in 0..MAX_COMMANDS_PER_STEP {
        let Some(command) = adapter.try_recv() else {
            break;
        };

        match command.payload {
            InboundPayload::SnapshotRequest => {
                let (seq, last_event) = observations.immediate();
                if game_state.board_id() != *last_board_id {
                    *last_board_id = game_state.board_id();
                    game_state.snapshot_board_into(snapshot);
                }
                game_state.snapshot_meta_into(snapshot);
                let observation = build_observation(seq, snapshot, last_event);
                adapter.send(OutboundMessage::ToClientObservationArc {
                    client_id: command.client_id,
                    obs: Arc::new(observation),
                });
            }
            InboundPayload::Command(payload) => {
                let result = apply_client_command(game_state, payload);
                observations.capture_core_event(game_state);

                match result {
                    Ok(()) => adapter.send(OutboundMessage::ToClientAck {
                        client_id: command.client_id,
                        ack: create_ack(command.seq, command.seq),
                    }),
                    Err(error) => adapter.send(OutboundMessage::ToClientError {
                        client_id: command.client_id,
                        err: create_error(
                            command.seq,
                            map_place_error_code(error),
                            error.message(),
                        ),
                    }),
                }
            }
        }
    }
}
