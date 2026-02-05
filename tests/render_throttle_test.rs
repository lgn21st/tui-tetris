use tui_tetris::term::RenderThrottle;

#[test]
fn render_throttle_renders_first_frame() {
    let mut t = RenderThrottle::new(250);
    assert!(t.should_render(0, 1, true));
}

#[test]
fn render_throttle_static_renders_on_change() {
    let mut t = RenderThrottle::new(250);
    assert!(t.should_render(0, 1, true));
    assert!(t.should_render(1, 2, true));
}

#[test]
fn render_throttle_static_throttles_when_unchanged() {
    let mut t = RenderThrottle::new(250);
    assert!(t.should_render(0, 1, true));
    assert!(!t.should_render(10, 1, true));
    assert!(!t.should_render(249, 1, true));
    assert!(t.should_render(250, 1, true));
}

#[test]
fn render_throttle_dynamic_always_renders() {
    let mut t = RenderThrottle::new(250);
    assert!(t.should_render(0, 1, false));
    assert!(t.should_render(1, 1, false));
    assert!(t.should_render(2, 1, false));
}

