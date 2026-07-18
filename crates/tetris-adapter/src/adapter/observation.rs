//! Projection of deterministic core snapshots into adapter observations.

use std::time::{SystemTime, UNIX_EPOCH};

use arrayvec::ArrayVec;

use crate::adapter::protocol::{
    ActivePieceSnapshot, BoardSnapshot, EventList, ObservationMessage, ObservationType,
    PieceKindLower, RotationLower, StateHash, TSpinLower, TimersSnapshot, TransitionEvent,
};
use tetris_core::core::GameSnapshot;
use tetris_core::types::{CoreLastEvent, TSpinKind};
use tetris_session::engine::replay::transition_hash;

/// Build an adapter observation from an immutable core snapshot.
pub fn build_observation(
    seq: u64,
    logical_step: u64,
    snap: &GameSnapshot,
    events: &[TransitionEvent],
) -> ObservationMessage {
    let core_events = events
        .iter()
        .map(|event| CoreLastEvent {
            locked: event.locked,
            lines_cleared: event.lines_cleared,
            line_clear_score: event.line_clear_score,
            tspin: event.tspin.map(|tspin| match tspin {
                TSpinLower::Mini => TSpinKind::Mini,
                TSpinLower::Full => TSpinKind::Full,
            }),
            combo: event.combo,
            back_to_back: event.back_to_back,
        })
        .collect::<ArrayVec<_, 4>>();
    // State identity excludes the separately transmitted logical step. Events
    // remain included because they are part of the represented observation.
    let state_hash = transition_hash(snap, 0, &core_events, &[]);

    let next_queue = std::array::from_fn(|index| PieceKindLower::from(snap.next_queue[index]));
    let active = snap.active.map(|piece| ActivePieceSnapshot {
        kind: PieceKindLower::from(piece.kind),
        rotation: RotationLower::from(piece.rotation),
        x: piece.x,
        y: piece.y,
    });

    ObservationMessage {
        msg_type: ObservationType::Observation,
        seq,
        ts: current_timestamp_ms(),
        logical_step,
        playable: snap.playable(),
        paused: snap.paused,
        game_over: snap.game_over,
        episode_id: snap.episode_id,
        seed: snap.seed,
        piece_id: snap.piece_id,
        step_in_piece: snap.step_in_piece,
        board: BoardSnapshot {
            width: 10,
            height: 20,
            cells: snap.board,
        },
        board_id: snap.board_id,
        active,
        ghost_y: snap.ghost_y,
        next: next_queue[0],
        next_queue,
        hold: snap.hold.map(PieceKindLower::from),
        can_hold: snap.can_hold,
        events: EventList(events.iter().cloned().collect()),
        state_hash: StateHash(state_hash),
        score: snap.score,
        level: snap.level,
        lines: snap.lines,
        timers: TimersSnapshot {
            drop_ms: snap.timers.drop_ms,
            lock_ms: snap.timers.lock_ms,
            line_clear_ms: snap.timers.line_clear_ms,
        },
    }
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
