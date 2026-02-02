use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tui_tetris::core::GameState;
use tui_tetris::term::{FrameBuffer, GameView, Viewport};

struct CountingAlloc;

static COUNT_ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT_ENABLED.load(Ordering::Relaxed) {
            let _ = layout;
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNT_ENABLED.load(Ordering::Relaxed) {
            let _ = (layout, new_size);
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        System.realloc(ptr, layout, new_size)
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
