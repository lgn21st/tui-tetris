use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell as Counter;

use tetris_core::core::{GameSnapshot, GameState};
use tetris_core::types::GameAction;
use tetris_terminal::term::kitty::{KittySupport, WellGeometry, WellRenderer};
use tetris_terminal::term::{Cell, FrameBuffer, GameView, Viewport, encode_diff_into};

struct CountingAlloc;

// Counting is per thread: the test harness allocates on its own thread while a
// test runs, and a process-wide counter would attribute those allocations to the
// measured hot path.
thread_local! {
    static COUNTING: Counter<bool> = const { Counter::new(false) };
    static ALLOCS: Counter<usize> = const { Counter::new(0) };
}

#[inline]
fn note_alloc() {
    let _ = COUNTING.try_with(|counting| {
        if counting.get() {
            let _ = ALLOCS.try_with(|allocs| allocs.set(allocs.get() + 1));
        }
    });
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            let _ = layout;
            note_alloc();
            System.alloc(layout)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            let _ = (layout, new_size);
            note_alloc();
            System.realloc(ptr, layout, new_size)
        }
    }
}

fn with_alloc_counting<F: FnOnce()>(f: F) -> usize {
    ALLOCS.with(|allocs| allocs.set(0));
    COUNTING.with(|counting| counting.set(true));
    f();
    COUNTING.with(|counting| counting.set(false));
    ALLOCS.with(|allocs| allocs.get())
}

#[test]
fn terminal_hot_paths_are_allocation_free_after_warmup() {
    let view = GameView::default();
    let viewport = Viewport::new(80, 24);
    let mut fb = FrameBuffer::new(viewport.width, viewport.height);

    let mut gs = GameState::new(1);
    gs.start();

    let mut snap = gs.snapshot();
    let mut last_board_id = gs.board_id();
    gs.snapshot_board_into(&mut snap);

    // Warm-up (resize/initial clears).
    if gs.board_id() != last_board_id {
        last_board_id = gs.board_id();
        gs.snapshot_board_into(&mut snap);
    }
    gs.snapshot_meta_into(&mut snap);
    view.render_into(&snap, viewport, &mut fb);

    let render_allocs = with_alloc_counting(|| {
        for _ in 0..200 {
            if gs.board_id() != last_board_id {
                last_board_id = gs.board_id();
                gs.snapshot_board_into(&mut snap);
            }
            gs.snapshot_meta_into(&mut snap);
            view.render_into(&snap, viewport, &mut fb);
        }
    });
    assert_eq!(render_allocs, 0);
    let mut prev = FrameBuffer::new(80, 24);
    let mut next = FrameBuffer::new(80, 24);
    next.set(
        4,
        4,
        Cell {
            ch: 'X',
            ..Cell::default()
        },
    );
    let mut out = Vec::with_capacity(64 * 1024);
    encode_diff_into(&prev, &next, &mut out).unwrap();
    std::mem::swap(&mut prev, &mut next);
    next.set(
        5,
        5,
        Cell {
            ch: 'Y',
            ..Cell::default()
        },
    );
    out.clear();
    encode_diff_into(&prev, &next, &mut out).unwrap();

    let allocs = with_alloc_counting(|| {
        let mut prev_x = 5u16;
        let mut prev_y = 5u16;
        for i in 0..200 {
            next.set(prev_x, prev_y, Cell::default());
            prev_x = (i % 40) as u16;
            prev_y = ((i / 40) % 12) as u16;
            next.set(
                prev_x,
                prev_y,
                Cell {
                    ch: 'Z',
                    ..Cell::default()
                },
            );
            out.clear();
            encode_diff_into(&prev, &next, &mut out).unwrap();
            std::mem::swap(&mut prev, &mut next);
        }
    });

    assert_eq!(allocs, 0);
}

#[test]
fn kitty_well_sync_is_allocation_free_after_warmup() {
    let geometry = WellGeometry::new(2, 1, (8, 17));
    let mut well = WellRenderer::new(
        KittySupport {
            tmux_passthrough: true,
        },
        geometry,
    );
    let mut out = Vec::with_capacity(1024 * 1024);
    let mut state = GameState::new(1);
    state.start();
    let mut snap = GameSnapshot::default();
    state.snapshot_into(&mut snap);

    // Warm up: first transmit grows the canvas, deflate and base64 buffers, and
    // a tmux envelope is longer than a bare APC.
    for _ in 0..8 {
        state.tick(16, false);
        state.snapshot_meta_into(&mut snap);
        out.clear();
        well.sync(&snap, geometry, (19, 1), &mut out).unwrap();
    }

    let allocs = with_alloc_counting(|| {
        for i in 0..600u32 {
            state.tick(16, false);
            if i % 40 == 0 {
                let _ = state.apply_action(GameAction::HardDrop);
            }
            state.snapshot_meta_into(&mut snap);
            out.clear();
            well.sync(&snap, geometry, (19, 1), &mut out).unwrap();
        }
    });

    assert_eq!(allocs, 0, "kitty sync must not allocate after warm-up");
}
