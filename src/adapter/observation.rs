//! Projection of deterministic core snapshots into adapter observations.

use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::adapter::protocol::{
    ActivePieceSnapshot, BoardSnapshot, LastEvent, ObservationMessage, ObservationType,
    PieceKindLower, RotationLower, StateHash, TimersSnapshot,
};
use crate::core::GameSnapshot;

/// Stable 64-bit FNV-1a hasher for deterministic `state_hash`.
///
/// `DefaultHasher` output is not guaranteed stable across Rust versions or platforms.
struct Fnv1aHasher {
    state: u64,
}

impl Fnv1aHasher {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    fn new() -> Self {
        Self {
            state: Self::OFFSET_BASIS,
        }
    }
}

impl Hasher for Fnv1aHasher {
    fn finish(&self) -> u64 {
        self.state
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state ^= byte as u64;
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
    }
}

/// Build an adapter observation from an immutable core snapshot.
pub fn build_observation(
    seq: u64,
    snap: &GameSnapshot,
    last_event: Option<LastEvent>,
) -> ObservationMessage {
    let mut hasher = Fnv1aHasher::new();
    snap.board_hash.hash(&mut hasher);
    snap.board_id.hash(&mut hasher);
    snap.active.hash(&mut hasher);
    snap.hold.hash(&mut hasher);
    snap.can_hold.hash(&mut hasher);
    snap.next_queue.hash(&mut hasher);
    snap.paused.hash(&mut hasher);
    snap.game_over.hash(&mut hasher);
    snap.episode_id.hash(&mut hasher);
    snap.piece_id.hash(&mut hasher);
    snap.step_in_piece.hash(&mut hasher);
    snap.seed.hash(&mut hasher);
    snap.score.hash(&mut hasher);
    snap.level.hash(&mut hasher);
    snap.lines.hash(&mut hasher);
    snap.timers.drop_ms.hash(&mut hasher);
    snap.timers.lock_ms.hash(&mut hasher);
    snap.timers.line_clear_ms.hash(&mut hasher);
    last_event.is_some().hash(&mut hasher);
    if let Some(event) = last_event.as_ref() {
        event.locked.hash(&mut hasher);
        event.lines_cleared.hash(&mut hasher);
        event.line_clear_score.hash(&mut hasher);
        event.tspin.hash(&mut hasher);
        event.combo.hash(&mut hasher);
        event.back_to_back.hash(&mut hasher);
    }

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
        last_event,
        state_hash: StateHash(hasher.finish()),
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
