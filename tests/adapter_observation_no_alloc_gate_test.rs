use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tui_tetris::adapter::server::build_observation;
use tui_tetris::core::GameState;

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
fn adapter_observation_build_and_serialize_is_allocation_free() {
    let mut gs = GameState::new(1);
    gs.start();

    // Pre-allocate a buffer large enough for observation JSON.
    let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024);
    let mut seq: u64 = 1;

    // Warm-up.
    let obs0 = build_observation(&gs, seq, gs.episode_id, gs.piece_id, gs.step_in_piece, None);
    buf.clear();
    serde_json::to_writer(&mut buf, &obs0).unwrap();

    let allocs = with_alloc_counting(|| {
        for _ in 0..200 {
            seq = seq.wrapping_add(1);
            let obs =
                build_observation(&gs, seq, gs.episode_id, gs.piece_id, gs.step_in_piece, None);
            buf.clear();
            serde_json::to_writer(&mut buf, &obs).unwrap();
        }
    });

    assert!(allocs == 0);
}
