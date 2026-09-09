use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tetris_core::core::GameState;
use tetris_terminal::term::{Cell, FrameBuffer, GameView, Viewport, encode_diff_into};

struct CountingAlloc;

static COUNT_ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_GATE: Mutex<()> = Mutex::new(());

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if COUNT_ENABLED.load(Ordering::Relaxed) {
                let _ = layout;
                ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            System.alloc(layout)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            if COUNT_ENABLED.load(Ordering::Relaxed) {
                let _ = (layout, new_size);
                ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            System.realloc(ptr, layout, new_size)
        }
    }
}

fn with_alloc_counting<F: FnOnce()>(f: F) -> usize {
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    COUNT_ENABLED.store(true, Ordering::Relaxed);
    f();
    COUNT_ENABLED.store(false, Ordering::Relaxed);
    ALLOC_COUNT.load(Ordering::Relaxed)
}

#[test]
fn term_game_view_render_is_allocation_free_after_warmup() {
    let _gate = ALLOC_GATE.lock().expect("alloc gate");
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

    let allocs = with_alloc_counting(|| {
        for _ in 0..200 {
            if gs.board_id() != last_board_id {
                last_board_id = gs.board_id();
                gs.snapshot_board_into(&mut snap);
            }
            gs.snapshot_meta_into(&mut snap);
            view.render_into(&snap, viewport, &mut fb);
        }
    });

    assert!(allocs == 0);
}

#[test]
fn encode_diff_into_is_allocation_free_after_warmup() {
    let _gate = ALLOC_GATE.lock().expect("alloc gate");
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
