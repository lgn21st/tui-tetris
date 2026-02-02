# Performance Guide

Performance optimization guide for tui-tetris.

## Current Performance Profile

### Measured Metrics (MacBook Pro M3)

| Operation | Current | Target | Priority |
|-----------|---------|--------|----------|
| Render frame | ~2ms | <1ms | P1 |
| Line clear | ~50μs | <10μs | P1 |
| Piece spawn | ~5μs | <5μs | P2 |
| Allocations/frame | (varies by adapter/input) | 0 (core hot paths) | P0 |
| Memory usage | ~50KB | ~30KB | P2 |

## Critical Issues

### Resolved: Board::clear_full_rows Allocates

**Status**: Fixed.

**Problem (historical)**: Called on every piece lock, allocated `Vec<usize>`

```rust
// Current (BAD)
pub fn clear_full_rows(&mut self) -> Vec<usize> {
    let mut cleared_rows = Vec::new();  // Allocates!
    // ...
    cleared_rows.reverse();             // May reallocate!
    cleared_rows
}
```

**Solution**: Use ArrayVec (stack allocation)

```rust
use arrayvec::ArrayVec;

pub fn clear_full_rows(&mut self) -> ArrayVec<usize, 4> {
    let mut cleared_rows = ArrayVec::new();  // Stack only, max 4 lines
    // ...
    cleared_rows
}
```

**Impact**: Eliminates heap allocation on hot path

### Resolved: Vec<Vec<Cell>> Double Indirection

**Status**: Fixed.

**Problem (historical)**: Poor cache locality, 200 pointer jumps

```rust
// Current (BAD)
cells: Vec<Vec<Cell>>  // Double indirection
```

**Solution**: Flat array

```rust
// Improved
cells: [Cell; 200]  // Single contiguous block

fn index(x: i8, y: i8) -> usize {
    (y as usize) * 10 + (x as usize)
}
```

**Impact**: 10-20% faster board operations

### Resolved: UI Full Re-render Every Frame

**Status**: Fixed (diff-based terminal rendering).

**Problem (historical)**: Rendered all 200 cells even when unchanged

**Solution**: Diff-based rendering

```rust
pub struct IncrementalRenderer {
    last_board: Board,
    last_active: Option<Tetromino>,
}

impl IncrementalRenderer {
    pub fn render(&mut self, state: &GameState, buf: &mut Buffer) {
        // Only render changed cells
        for pos in state.board.diff(&self.last_board) {
            render_cell(buf, pos, state.board.get(pos));
        }
        
        // Clear old active piece position
        if let Some(last) = self.last_active {
            if state.active != Some(last) {
                clear_piece(buf, last);
            }
        }
        
        // Draw new active piece
        if let Some(active) = state.active {
            draw_piece(buf, active);
        }
        
        self.last_board = state.board.clone();
        self.last_active = state.active;
    }
}
```

**Impact**: 80%+ reduction in render time when piece is falling

## Optimization Techniques

### 1. Zero-Allocation Hot Paths

Mark functions with `#[inline]` and ensure no allocations:

Repo gate:
- `cargo test --test no_alloc_gate_test`
- `cargo test --test input_no_alloc_gate_test`
- `cargo test --test adapter_observation_no_alloc_gate_test`
- `cargo test --test term_no_alloc_gate_test`

```rust
#[inline]
pub fn tick(&mut self, elapsed_ms: u32, soft_drop: bool) {
    // No Vec::new(), no String creation, no Box
}
```

### 2. Pre-computed Lookup Tables

```rust
// Pre-compute piece shapes at compile time
const SHAPES: [[[i8; 2]; 4]; 7] = [
    [[0,1], [1,1], [2,1], [3,1]],  // I North
    // ... all rotations
];

pub fn get_shape(kind: PieceKind, rotation: Rotation) -> [(i8, i8); 4] {
    SHAPES[kind as usize][rotation as usize]
}
```

### 3. Bitboards (Advanced)

For ultimate performance, use bit manipulation:

```rust
// Represent board as 20 u16s (one per row)
pub struct BitBoard {
    rows: [u16; 20],  // Each bit is a cell
}

impl BitBoard {
    pub fn is_valid(&self, shape: u16, x: i8, y: i8) -> bool {
        let row_mask = shape << x;
        self.rows[y as usize] & row_mask == 0
    }
    
    pub fn clear_lines(&mut self) -> u32 {
        // SIMD-friendly row compression
        // ...
    }
}
```

### 4. Lock-Free Rendering

Use double buffering for thread-safe rendering:

```rust
pub struct RenderBuffer {
    front: Vec<Cell>,  // UI reads this
    back: Vec<Cell>,   // Game writes this
}

impl RenderBuffer {
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.front, &mut self.back);
    }
}
```

## Profiling

### Setup Criterion Benchmarks

```rust
// benches/game_logic.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tui_tetris::core::GameState;

fn tick_benchmark(c: &mut Criterion) {
    let mut state = GameState::new(1);
    state.start();
    
    c.bench_function("tick_16ms", |b| {
        b.iter(|| state.tick(black_box(16), false))
    });
}

fn line_clear_benchmark(c: &mut Criterion) {
    let mut board = Board::new();
    // Fill bottom 4 rows
    for y in 16..20 {
        for x in 0..10 {
            board.set(x, y, Some(PieceKind::I));
        }
    }
    
    c.bench_function("clear_4_lines", |b| {
        b.iter(|| {
            let mut board = board.clone();
            board.clear_full_rows()
        })
    });
}

criterion_group!(benches, tick_benchmark, line_clear_benchmark);
criterion_main!(benches);
```

### Run Benchmarks

```bash
cargo bench
```

### Memory Profiling

```bash
# Use dhat or heaptrack
cargo build --release
heaptrack ./target/release/tui-tetris
```

## Checklist

### Before Release

- [x] Zero allocations in core tick/apply_action hot paths (see `tests/no_alloc_gate_test.rs`)
- [ ] Zero allocations in end-to-end runner (input + adapter + render)
- [ ] All hot paths marked `#[inline]`
- [ ] Benchmarks passing (<1ms per frame)
- [ ] Memory usage <50KB steady-state
- [ ] No memory leaks in long-running test (1 hour)

### Tools

- `cargo bench` - Performance benchmarks
- `cargo flamegraph` - Visual profiling
- `heaptrack` - Memory profiling
- `perf` - Linux performance counters

## References

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Criterion.rs Guide](https://bheisler.github.io/criterion.rs/book/)
- [ArrayVec Documentation](https://docs.rs/arrayvec)
