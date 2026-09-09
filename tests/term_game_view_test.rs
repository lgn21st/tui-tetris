use tetris_core::core::{GameState, get_shape};
use tetris_core::types::PieceKind;
use tetris_terminal::term::{
    AdapterStatusView, AnchorY, FrameBuffer, GameView, GameViewModel, HudOverlay, HudOverlayValue,
    Rgb, Viewport,
};

fn is_solid_mino(cell: tetris_terminal::term::Cell) -> bool {
    cell.ch == ' ' && cell.style.fg == cell.style.bg && {
        let bg = cell.style.bg;
        bg != Rgb::new(16, 16, 16) && bg != Rgb::new(28, 28, 28) && bg != Rgb::new(42, 42, 42)
    }
}

fn is_hud_mino(cell: tetris_terminal::term::Cell) -> bool {
    if is_solid_mino(cell) {
        return true;
    }
    matches!(cell.ch, '▀' | '▄')
        && cell.style.fg != Rgb::new(16, 16, 16)
        && cell.style.fg != cell.style.bg
}

fn dump(fb: &FrameBuffer) -> String {
    let mut all = String::new();
    for y in 0..fb.height() {
        for x in 0..fb.width() {
            all.push(fb.get(x, y).unwrap().ch);
        }
        all.push('\n');
    }
    all
}

fn panel_minos(fb: &FrameBuffer) -> usize {
    let mut n = 0;
    for y in 0..fb.height() {
        for x in 40..fb.width() {
            if is_hud_mino(fb.get(x, y).unwrap()) {
                n += 1;
            }
        }
    }
    n
}

fn observe_overlay() -> HudOverlay {
    HudOverlay::from_rows([
        ("MODE", HudOverlayValue::text("OBSERVE")),
        ("TARGET", HudOverlayValue::text("127.0.0.1:7780")),
        ("STATE", HudOverlayValue::text("PLAY")),
        ("EP", HudOverlayValue::text("7 PIECE 9 STEP 1")),
        ("SEED", HudOverlayValue::U32(123)),
    ])
}

#[test]
fn squarest_cell_size_picks_1x1_for_square_fonts_and_2x1_for_classic_cells() {
    assert_eq!(tetris_terminal::term::squarest_cell_size(8, 16), (2, 1));
    assert_eq!(tetris_terminal::term::squarest_cell_size(12, 12), (1, 1));
    assert_eq!(GameView::from_cell_pixels(8, 16).cell_size(), (2, 1));
    assert_eq!(GameView::from_cell_pixels(12, 12).cell_size(), (1, 1));
}

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

    let rim = Rgb::new(46, 46, 46);
    let page = Rgb::new(16, 16, 16);
    assert_eq!(fb.get(0, 0).unwrap().ch, '┌');
    assert_eq!(fb.get(21, 0).unwrap().ch, '┐');
    assert_eq!(fb.get(0, 21).unwrap().ch, '└');
    assert_eq!(fb.get(21, 21).unwrap().ch, '┘');
    let top = fb.get(0, 0).unwrap();
    assert_eq!(top.style.fg, rim);
    assert_eq!(top.style.bg, page);
}

#[test]
fn term_view_well_has_no_glyph_grid() {
    let snap = GameState::new(1).snapshot();
    let fb = GameView::default().render(&snap, Viewport::new(22, 22));
    let all = dump(&fb);
    assert!(
        !all.contains('·'),
        "empty well must not stamp a dot on every cell"
    );
    let interior = fb.get(1, 1).unwrap();
    assert_eq!(interior.ch, ' ');
    assert_eq!(interior.style.bg, Rgb::new(28, 28, 28));
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

    // Inside border: (1,1) origin. Each cell is 2 chars wide, filled by
    // matching fg/bg so the mino is a square independent of glyph metrics.
    let x0 = 1;
    let y0 = 1 + 19;
    let left = fb.get(x0, y0).unwrap();
    let right = fb.get(x0 + 1, y0).unwrap();
    assert!(is_solid_mino(left));
    assert_eq!(left, right);
    assert_eq!(left.style.bg, Rgb::new(79, 209, 196));
}

#[test]
fn term_view_ghost_is_neutral_gray() {
    let mut gs = GameState::new(1);
    gs.start();
    let snap = gs.snapshot();
    let active = snap.active.expect("started game has an active piece");
    let ghost_y = snap.ghost_y.expect("empty well has a ghost");
    assert_ne!(ghost_y, active.y);

    let fb = GameView::default().render(&snap, Viewport::new(22, 22));
    let (dx, dy) = get_shape(active.kind, active.rotation)[0];
    let cell = fb
        .get(1 + (active.x + dx) as u16 * 2, 1 + (ghost_y + dy) as u16)
        .unwrap();

    assert_eq!(cell.style.bg, Rgb::new(42, 42, 42));
    assert_eq!(cell.style.fg, Rgb::new(42, 42, 42));
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

    let all = dump(&fb);
    assert!(all.contains("SCORE"));
    assert!(all.contains("HOLD"));
    assert!(all.contains("NEXT"));

    assert!(
        panel_minos(&fb) >= 8,
        "hold/next minos should paint at preview scale, got {}",
        panel_minos(&fb)
    );
}

#[test]
fn term_view_hides_ai_panel_without_adapter_status() {
    let mut gs = GameState::new(1);
    gs.start();
    let snap = gs.snapshot();
    let view = GameView::default();
    let fb = view.render(&snap, Viewport::new(60, 22));
    let all = dump(&fb);

    assert!(!all.contains("AI"));
    assert!(!all.contains("CONN"));
    assert!(!all.contains("ST"));
    assert!(!all.contains("CTRL"));
    assert!(!all.contains("PORT"));
    assert!(!all.contains("PID"));
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
    let all = dump(&fb);

    assert!(all.contains("PID"));
    assert!(all.contains("4242"));
    assert!(all.contains("PORT"));
    assert!(all.contains("7777"));
    assert!(all.contains("CONN"));
    assert!(all.contains("ST"));
    assert!(all.contains("CTRL"));
    let text = dump(&fb);
    let lines: Vec<&str> = text.lines().take(5).collect();
    assert!(lines[0].contains("CONN"), "row 0: {}", lines[0]);
    assert!(lines[1].contains("ST"), "row 1: {}", lines[1]);
    assert!(lines[2].contains("CTRL"), "row 2: {}", lines[2]);
    assert!(lines[3].contains("PORT"), "row 3: {}", lines[3]);
    assert!(lines[4].contains("PID"), "row 4: {}", lines[4]);
}

#[test]
fn term_view_preview_minos_pack_to_half_size_squares() {
    let mut snap = GameState::new(1).snapshot();
    snap.board[19][0] = 1;
    snap.active = None;
    snap.ghost_y = None;
    snap.hold = Some(PieceKind::O);
    snap.next_queue = [PieceKind::O; 5];

    let fb = GameView::default().render(&snap, Viewport::new(60, 24));

    // Frame is 22×22, centered in 60×24: start=(19,1). Well cell (0,19) is 2-wide.
    let well_x = 20u16;
    let well_y = 21u16;
    let well_left = fb.get(well_x, well_y).unwrap();
    let well_right = fb.get(well_x + 1, well_y).unwrap();
    assert!(is_solid_mino(well_left));
    assert_eq!(well_left, well_right, "well minos stay cell_w=2");

    // panel_x=19+22+2=43. HOLD label at y=7, preview origin at y=8.
    // O is 2×2 minos packed into 2 cols × 1 row of filled cells.
    let px = 43u16;
    let py = 8u16;
    let left = fb.get(px, py).unwrap();
    let right = fb.get(px + 1, py).unwrap();
    assert!(is_solid_mino(left), "packed O left cell");
    assert!(is_solid_mino(right), "packed O right cell");
    assert_eq!(left.style.bg, Rgb::new(245, 224, 94));
    assert!(
        !is_hud_mino(fb.get(px + 2, py).unwrap()),
        "preview O is 2 columns, not well-scale 4"
    );
    assert!(
        !is_hud_mino(fb.get(px, py + 1).unwrap()),
        "preview O packs both mino rows into one terminal row"
    );
}

#[test]
fn term_view_preview_t_piece_uses_half_blocks() {
    let mut snap = GameState::new(1).snapshot();
    snap.active = None;
    snap.ghost_y = None;
    snap.hold = Some(PieceKind::T);
    snap.next_queue = [PieceKind::I; 5];

    let fb = GameView::default().render(&snap, Viewport::new(60, 24));
    let px = 43u16;
    let py = 8u16;
    let t = Rgb::new(158, 122, 235);
    let page = Rgb::new(16, 16, 16);
    let left = fb.get(px, py).unwrap();
    let mid = fb.get(px + 1, py).unwrap();
    let right = fb.get(px + 2, py).unwrap();
    assert_eq!(left.ch, '▄');
    assert_eq!(left.style.fg, t);
    assert_eq!(left.style.bg, page);
    assert!(is_solid_mino(mid));
    assert_eq!(mid.style.bg, t);
    assert_eq!(right.ch, '▄');
    assert_eq!(right.style.fg, t);
    assert_eq!(right.style.bg, page);
}

#[test]
fn term_view_shows_five_next_pieces_with_or_without_overlay() {
    let mut snap = GameState::new(1).snapshot();
    snap.active = None;
    snap.ghost_y = None;
    snap.hold = None;
    snap.next_queue = [PieceKind::O; 5];

    let view = GameView::default();
    let vp = Viewport::new(60, 40);
    let without = view.render(&snap, vp);
    let adapter = AdapterStatusView {
        enabled: true,
        client_count: 1,
        controller_id: Some(1),
        streaming_count: 1,
        pid: 7,
        listen_addr: Some("127.0.0.1:7777".parse().unwrap()),
    };
    let with_adapter = view.render_with_adapter(&snap, Some(&adapter), vp);
    let mut with_observe = FrameBuffer::new(vp.width, vp.height);
    view.render_model_into(
        &GameViewModel::new(snap, Some(observe_overlay())),
        vp,
        &mut with_observe,
    );

    let local = panel_minos(&without);
    let adapter_count = panel_minos(&with_adapter);
    let observe_count = panel_minos(&with_observe);
    assert_eq!(
        local, 10,
        "local HUD shows five packed O previews (5 × 2 cells)"
    );
    assert_eq!(
        adapter_count, 10,
        "adapter overlay still shows the full next queue"
    );
    assert_eq!(
        observe_count, 10,
        "observe overlay still shows the full next queue"
    );
}

#[test]
fn term_view_observe_overlay_uses_the_same_five_row_slot() {
    let snap = GameState::new(1).snapshot();
    let mut fb = FrameBuffer::new(60, 24);
    GameView::default().render_model_into(
        &GameViewModel::new(snap, Some(observe_overlay())),
        Viewport::new(60, 24),
        &mut fb,
    );
    let text = dump(&fb);
    let lines: Vec<&str> = text.lines().take(5).collect();
    assert!(
        lines[0].contains("MODE") && lines[0].contains("OBSERVE"),
        "row 0: {}",
        lines[0]
    );
    assert!(
        lines[1].contains("TARGET") && lines[1].contains("127.0.0.1:7780"),
        "row 1: {}",
        lines[1]
    );
    assert!(
        lines[2].contains("STATE") && lines[2].contains("PLAY"),
        "row 2: {}",
        lines[2]
    );
    assert!(
        lines[3].contains("EP") && lines[3].contains("PIECE"),
        "row 3: {}",
        lines[3]
    );
    assert!(
        lines[4].contains("SEED") && lines[4].contains("123"),
        "row 4: {}",
        lines[4]
    );
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
    let all = dump(&fb);

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

#[test]
fn term_view_hud_uses_guideline_level_and_chain_status() {
    let mut snap = GameState::new(1).snapshot();
    snap.level = 2;
    snap.lines = 24;
    snap.score = 800;
    snap.combo = 3;
    snap.back_to_back = true;
    snap.hold = Some(PieceKind::I);

    let fb = GameView::default().render(&snap, Viewport::new(60, 24));
    let all = dump(&fb);
    assert!(
        all.contains("LEVEL 3"),
        "HUD shows guideline level (engine+1): {all}"
    );
    assert!(all.contains("COMBO"));
    assert!(all.contains("B2B"));
}

#[test]
fn term_view_hides_idle_combo_and_b2b() {
    let mut snap = GameState::new(1).snapshot();
    snap.combo = -1;
    snap.back_to_back = false;
    let fb = GameView::default().render(&snap, Viewport::new(60, 22));
    let all = dump(&fb);
    assert!(!all.contains("COMBO"));
    assert!(!all.contains("B2B"));
}

#[test]
fn term_view_paused_overlay_includes_resume_hint() {
    let mut snap = GameState::new(1).snapshot();
    snap.paused = true;
    let fb = GameView::default().render(&snap, Viewport::new(40, 22));
    let all = dump(&fb);
    assert!(all.contains("PAUSED"));
    assert!(all.contains("P resume") || all.contains("resume"));
}

#[test]
fn term_view_game_over_overlay_includes_score_and_retry() {
    let mut snap = GameState::new(1).snapshot();
    snap.game_over = true;
    snap.score = 1600;
    let fb = GameView::default().render(&snap, Viewport::new(40, 22));
    let all = dump(&fb);
    assert!(all.contains("GAME OVER"));
    assert!(all.contains("1600"));
    assert!(all.contains("R retry") || all.contains("retry"));
}

#[test]
fn term_view_help_line_on_tall_viewport() {
    let snap = GameState::new(1).snapshot();
    let fb = GameView::default().render(&snap, Viewport::new(60, 30));
    let all = dump(&fb);
    assert!(all.contains("pause") || all.contains("PAUSE") || all.contains("hold"));
}
