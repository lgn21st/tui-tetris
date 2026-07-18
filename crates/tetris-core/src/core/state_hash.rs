//! Stable hashing for deterministic snapshots and replay records.

use crate::core::GameSnapshot;
use crate::types::{CoreLastEvent, PieceKind, Rotation, TSpinKind};

const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const PRIME: u64 = 0x100000001b3;

fn write(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(PRIME);
    }
}

fn piece(piece: PieceKind) -> u8 {
    match piece {
        PieceKind::I => 1,
        PieceKind::O => 2,
        PieceKind::T => 3,
        PieceKind::S => 4,
        PieceKind::Z => 5,
        PieceKind::J => 6,
        PieceKind::L => 7,
    }
}

fn rotation(rotation: Rotation) -> u8 {
    match rotation {
        Rotation::North => 0,
        Rotation::East => 1,
        Rotation::South => 2,
        Rotation::West => 3,
    }
}

/// Hashes every deterministic snapshot field plus the transient step event.
pub fn stable_state_hash(snapshot: &GameSnapshot, event: Option<CoreLastEvent>) -> u64 {
    let mut hash = OFFSET_BASIS;
    write(&mut hash, &snapshot.board_hash.to_le_bytes());
    write(&mut hash, &snapshot.board_id.to_le_bytes());
    write(&mut hash, &[u8::from(snapshot.active.is_some())]);
    if let Some(active) = snapshot.active {
        write(
            &mut hash,
            &[
                piece(active.kind),
                rotation(active.rotation),
                active.x as u8,
                active.y as u8,
            ],
        );
    }
    write(&mut hash, &[u8::from(snapshot.hold.is_some())]);
    if let Some(hold) = snapshot.hold {
        write(&mut hash, &[piece(hold)]);
    }
    write(&mut hash, &[u8::from(snapshot.can_hold)]);
    for next in snapshot.next_queue {
        write(&mut hash, &[piece(next)]);
    }
    write(
        &mut hash,
        &[u8::from(snapshot.paused), u8::from(snapshot.game_over)],
    );
    for value in [
        snapshot.episode_id,
        snapshot.piece_id,
        snapshot.step_in_piece,
        snapshot.seed,
        snapshot.score,
        snapshot.level,
        snapshot.lines,
        snapshot.timers.drop_ms,
        snapshot.timers.lock_ms,
        snapshot.timers.line_clear_ms,
    ] {
        write(&mut hash, &value.to_le_bytes());
    }
    write(&mut hash, &[u8::from(event.is_some())]);
    if let Some(event) = event {
        write(&mut hash, &[u8::from(event.locked)]);
        write(&mut hash, &event.lines_cleared.to_le_bytes());
        write(&mut hash, &event.line_clear_score.to_le_bytes());
        write(&mut hash, &[u8::from(event.tspin.is_some())]);
        if let Some(tspin) = event.tspin {
            let code = match tspin {
                TSpinKind::None => 0,
                TSpinKind::Mini => 1,
                TSpinKind::Full => 2,
            };
            write(&mut hash, &[code]);
        }
        write(&mut hash, &event.combo.to_le_bytes());
        write(&mut hash, &[u8::from(event.back_to_back)]);
    }
    hash
}
