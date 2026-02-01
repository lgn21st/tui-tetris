use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tui_tetris::core::{Board, GameState};
use tui_tetris::types::PieceKind;

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

fn bench_piece_spawn(c: &mut Criterion) {
    let mut state = GameState::new(12345);
    state.start();

    c.bench_function("spawn_piece", |b| {
        b.iter(|| {
            state.spawn_piece();
        })
    });
}

fn bench_try_move(c: &mut Criterion) {
    let mut state = GameState::new(12345);
    state.start();

    c.bench_function("try_move", |b| {
        b.iter(|| {
            state.try_move(1, 0);
        })
    });
}

fn bench_try_rotate(c: &mut Criterion) {
    let mut state = GameState::new(12345);
    state.start();

    c.bench_function("try_rotate", |b| {
        b.iter(|| {
            state.try_rotate(true);
        })
    });
}

criterion_group!(
    benches,
    bench_tick,
    bench_line_clear,
    bench_piece_spawn,
    bench_try_move,
    bench_try_rotate
);
criterion_main!(benches);
