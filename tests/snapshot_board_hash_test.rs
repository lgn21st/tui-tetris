use tui_tetris::core::{GameSnapshot, GameState};
use tui_tetris::types::GameAction;

fn fnv1a64_bytes(bytes: impl Iterator<Item = u8>) -> u64 {
    // FNV-1a 64-bit.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x00000100000001B3);
    }
    h
}

fn fnv1a64_board(board: &[[u8; 10]; 20]) -> u64 {
    fnv1a64_bytes(board.iter().flat_map(|row| row.iter().copied()))
}

#[test]
fn snapshot_board_into_sets_board_hash() {
    let mut gs = GameState::new(1);
    gs.start();

    let mut snap = GameSnapshot::default();
    gs.snapshot_board_into(&mut snap);

    assert_eq!(snap.board_hash, fnv1a64_board(&snap.board));

    let _ = gs.apply_action(GameAction::HardDrop);
    let _ = gs.tick(1000, false);

    gs.snapshot_board_into(&mut snap);
    assert_eq!(snap.board_hash, fnv1a64_board(&snap.board));
}

#[test]
fn snapshot_meta_into_does_not_change_board_hash() {
    let mut gs = GameState::new(1);
    gs.start();

    let mut snap = GameSnapshot::default();
    gs.snapshot_board_into(&mut snap);
    let before = snap.board_hash;

    gs.snapshot_meta_into(&mut snap);
    assert_eq!(snap.board_hash, before);
    assert_eq!(snap.board_hash, fnv1a64_board(&snap.board));
}

