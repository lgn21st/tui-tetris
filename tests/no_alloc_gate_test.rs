use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tui_tetris::core::GameState;
use tui_tetris::types::GameAction;

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
fn core_hot_paths_do_not_allocate() {
    // Setup (outside counting) so one-time allocations don't trip the gate.
    let mut gs = GameState::new(1);
    gs.start();

    // Warm-up.
    let _ = gs.tick(16, false);
    let _ = gs.apply_action(GameAction::MoveLeft);

    let allocs = with_alloc_counting(|| {
        // Tick should be allocation-free.
        for _ in 0..200 {
            let _ = gs.tick(16, false);
        }

        // Common actions should be allocation-free.
        for _ in 0..50 {
            let _ = gs.apply_action(GameAction::MoveLeft);
            let _ = gs.apply_action(GameAction::MoveRight);
            let _ = gs.apply_action(GameAction::RotateCw);
            let _ = gs.apply_action(GameAction::RotateCcw);
        }

        // Hard drop drives lock/line-clear and piece spawning paths.
        for _ in 0..25 {
            let _ = gs.apply_action(GameAction::HardDrop);
            // Advance timers so line-clear pause drains deterministically.
            let _ = gs.tick(1000, false);
            if gs.game_over {
                let _ = gs.apply_action(GameAction::Restart);
            }
        }
    });

    assert!(allocs == 0);
}
