use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tui_tetris::adapter::server::build_observation;
use tui_tetris::core::{Board, GameSnapshot, GameState};
use tui_tetris::term::{FrameBuffer, GameView, Viewport};
use tui_tetris::types::{GameAction, PieceKind};

fn bench_tick(c: &mut Criterion) {
    let mut state = GameState::new(12345);
    state.start();

    c.bench_function("game_tick_16ms", |b| {
        b.iter(|| {
            state.tick(black_box(16), false);
        })
    });
}

fn bench_line_clear(c: &mut Criterion) {
    c.bench_function("clear_4_lines", |b| {
        b.iter(|| {
            let mut board = Board::new();
            // Fill bottom 4 rows
            for y in 16..20 {
                for x in 0..10 {
                    board.set(x, y, Some(PieceKind::I));
                }
            }
            board.clear_full_rows();
        })
    });
}

fn bench_snapshot_meta_into(c: &mut Criterion) {
    let mut state = GameState::new(12345);
    state.start();
    let mut snap = GameSnapshot::default();
    state.snapshot_board_into(&mut snap);

    c.bench_function("snapshot_meta_into", |b| {
        b.iter(|| {
            state.snapshot_meta_into(black_box(&mut snap));
        })
    });
}

fn bench_snapshot_board_into(c: &mut Criterion) {
    let mut state = GameState::new(12345);
    state.start();
    let mut snap = GameSnapshot::default();

    // Force a board change so board_id is non-zero.
    let _ = state.apply_action(GameAction::HardDrop);
    let _ = state.tick(1000, false);

    c.bench_function("snapshot_board_into", |b| {
        b.iter(|| {
            state.snapshot_board_into(black_box(&mut snap));
        })
    });
}

fn bench_build_observation_and_serialize(c: &mut Criterion) {
    let mut state = GameState::new(12345);
    state.start();
    let mut snap = GameSnapshot::default();
    state.snapshot_into(&mut snap);
    let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024);
    let mut seq: u64 = 1;

    c.bench_function("build_observation+to_writer", |b| {
        b.iter(|| {
            seq = seq.wrapping_add(1);
            state.snapshot_into(&mut snap);
            let obs = build_observation(seq, &snap, None);
            buf.clear();
            serde_json::to_writer(&mut buf, &obs).unwrap();
            black_box(buf.len())
        })
    });
}

fn bench_render_into(c: &mut Criterion) {
    let mut state = GameState::new(12345);
    state.start();
    let mut snap = GameSnapshot::default();
    state.snapshot_into(&mut snap);
    let view = GameView::default();
    let viewport = Viewport::new(80, 24);
    let mut fb = FrameBuffer::new(viewport.width, viewport.height);

    c.bench_function("render_into", |b| {
        b.iter(|| {
            state.snapshot_meta_into(&mut snap);
            view.render_into(black_box(&snap), viewport, black_box(&mut fb));
        })
    });
}

criterion_group!(
    benches,
    bench_tick,
    bench_line_clear,
    bench_snapshot_meta_into,
    bench_snapshot_board_into,
    bench_build_observation_and_serialize,
    bench_render_into
);
criterion_main!(benches);
