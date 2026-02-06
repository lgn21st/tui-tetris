use tui_tetris::core::GameState;
use tui_tetris::term::{AdapterStatusView, AnchorY, GameView, Viewport};
use tui_tetris::types::PieceKind;

#[test]
fn term_view_renders_border_corners() {
    let state = GameState::new(1);
    let snap = state.snapshot();
    let view = GameView::default();

    // With cell_w=2 and cell_h=1:
    // board pixels = 10*2 by 20*1 => 20x20
    // plus border => 22x22
    let vp = Viewport::new(22, 22);
    let fb = view.render(&snap, vp);

    assert_eq!(fb.get(0, 0).unwrap().ch, '┌');
    assert_eq!(fb.get(21, 0).unwrap().ch, '┐');
    assert_eq!(fb.get(0, 21).unwrap().ch, '└');
    assert_eq!(fb.get(21, 21).unwrap().ch, '┘');
}

#[test]
fn term_view_renders_locked_cell_as_two_chars_wide() {
    let mut snap = GameState::new(1).snapshot();
    // Put a locked I block at bottom-left.
    snap.board[19][0] = 1;
    snap.active = None;
    snap.ghost_y = None;

    let view = GameView::default();
    let vp = Viewport::new(22, 22);
    let fb = view.render(&snap, vp);

    // Inside border: (1,1) origin. Each cell is 2 chars wide.
    let x0 = 1;
    let y0 = 1 + 19;
    assert_eq!(fb.get(x0, y0).unwrap().ch, '█');
    assert_eq!(fb.get(x0 + 1, y0).unwrap().ch, '█');
}

#[test]
fn term_view_draws_side_panel_when_wide_enough() {
    let mut gs = GameState::new(1);
    gs.start();
    let mut snap = gs.snapshot();
    snap.score = 1234;
    snap.level = 2;
    snap.lines = 10;
    snap.hold = Some(PieceKind::T);

    let view = GameView::default();
    // Wider than the 22x22 board frame to allow a panel.
    let fb = view.render(&snap, Viewport::new(60, 22));

    // Expect the word SCORE to be present somewhere (board is vertically centered).
    let mut all = String::new();
    for y in 0..fb.height() {
        for x in 0..fb.width() {
            all.push(fb.get(x, y).unwrap().ch);
        }
        all.push('\n');
    }
    assert!(all.contains("SCORE"));
}

#[test]
fn term_view_centers_board_by_default_on_tall_viewports() {
    let state = GameState::new(1);
    let snap = state.snapshot();
    let view = GameView::default();

    // Board frame is 22 rows tall (20 + border).
    let vp = Viewport::new(22, 30);
    let fb = view.render(&snap, vp);

    // start_y = (30 - 22) / 2 = 4 => top-left corner at (0,4).
    assert_eq!(fb.get(0, 4).unwrap().ch, '┌');
}

#[test]
fn term_view_can_anchor_board_to_top() {
    let state = GameState::new(1);
    let snap = state.snapshot();
    let view = GameView::default().with_anchor_y(AnchorY::Top);

    let vp = Viewport::new(22, 30);
    let fb = view.render(&snap, vp);

    assert_eq!(fb.get(0, 0).unwrap().ch, '┌');
}

#[test]
fn term_view_renders_adapter_pid_and_port_when_enabled() {
    let mut gs = GameState::new(1);
    gs.start();
    let snap = gs.snapshot();
    let view = GameView::default();

    let adapter = AdapterStatusView {
        enabled: true,
        client_count: 2,
        controller_id: Some(1),
        streaming_count: 1,
        pid: 4242,
        listen_addr: Some("127.0.0.1:7777".parse().unwrap()),
    };

    let fb = view.render_with_adapter(&snap, Some(&adapter), Viewport::new(60, 22));

    let mut all = String::new();
    for y in 0..fb.height() {
        for x in 0..fb.width() {
            all.push(fb.get(x, y).unwrap().ch);
        }
        all.push('\n');
    }

    assert!(all.contains("PID"));
    assert!(all.contains("4242"));
    assert!(all.contains("PORT"));
    assert!(all.contains("7777"));
    assert!(all.contains("CONN"));
    assert!(all.contains("ST"));
    assert!(all.contains("CTRL"));
}

#[test]
fn term_view_renders_adapter_port_when_space_allows() {
    let mut gs = GameState::new(1);
    gs.start();
    let snap = gs.snapshot();
    let view = GameView::default();

    let adapter = AdapterStatusView {
        enabled: true,
        client_count: 2,
        controller_id: Some(1),
        streaming_count: 1,
        pid: 4242,
        listen_addr: Some("127.0.0.1:7777".parse().unwrap()),
    };

    let fb = view.render_with_adapter(&snap, Some(&adapter), Viewport::new(80, 22));

    let mut all = String::new();
    for y in 0..fb.height() {
        for x in 0..fb.width() {
            all.push(fb.get(x, y).unwrap().ch);
        }
        all.push('\n');
    }

    assert!(all.contains("PORT"));
    assert!(all.contains("7777"));
}

#[test]
fn adapter_status_code_mapping_is_stable() {
    let base = AdapterStatusView {
        enabled: false,
        client_count: 0,
        controller_id: None,
        streaming_count: 0,
        pid: 1,
        listen_addr: None,
    };
    assert_eq!(base.status_code(), 0);

    let mut st = base;
    st.enabled = true;
    assert_eq!(st.status_code(), 1);

    st.client_count = 1;
    assert_eq!(st.status_code(), 2);

    st.controller_id = Some(7);
    assert_eq!(st.status_code(), 3);

    st.streaming_count = 1;
    assert_eq!(st.status_code(), 4);
}
