use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossterm::event::KeyCode;

use tui_tetris::adapter::server::build_observation;
use tui_tetris::engine::session::{SessionRuntime, StepInput};
use tui_tetris::input::InputHandler;
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
fn e2e_hot_path_is_allocation_free_without_io() {
    let mut session = SessionRuntime::new(1);

    let mut ih = InputHandler::new();
    let _ = ih.handle_key_press(KeyCode::Left);

    let view = GameView::default();
    let viewport = Viewport::new(80, 24);
    let mut fb = FrameBuffer::new(viewport.width, viewport.height);

    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut seq: u64 = 1;
    // Warm-up: allow any lazy init/resizes.
    let actions = ih.update(16);
    let mut input = StepInput::default();
    input.local.extend(actions);
    let _ = session.transition(&input);
    let mut snap = *session.snapshot();
    let obs0 = build_observation(seq, 0, &snap, &[]);
    buf.clear();
    serde_json::to_writer(&mut buf, &obs0).unwrap();
    view.render_into(&snap, viewport, &mut fb);

    let allocs = with_alloc_counting(|| {
        for _ in 0..500 {
            // Input -> actions.
            let actions = ih.update(16);
            let mut input = StepInput::default();
            input.local.extend(actions);
            let _ = session.transition(&input);

            // Observation build + serialize to preallocated buffer.
            seq = seq.wrapping_add(1);
            snap = *session.snapshot();
            let obs = build_observation(seq, 0, &snap, &[]);
            buf.clear();
            serde_json::to_writer(&mut buf, &obs).unwrap();

            // Render into preallocated framebuffer.
            view.render_into(&snap, viewport, &mut fb);
        }
    });

    assert!(allocs == 0);

    let transition_allocs = with_alloc_counting(|| {
        let idle = StepInput::default();
        for _ in 0..100_000 {
            std::hint::black_box(session.transition(&idle));
        }
    });
    assert_eq!(transition_allocs, 0, "100k-step session soak allocated");
    assert!(session.logical_step() >= 100_501);
}
