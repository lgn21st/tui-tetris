use tetris_core::core::GameState;
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
    assert_eq!(GameView::new(2, 1).cell_size(), (2, 1));
    assert_eq!(GameView::new(1, 1).cell_size(), (1, 1));
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

    assert_eq!(
        panel_minos(&fb),
        0,
        "HOLD/NEXT pieces are pixel layers now, not character cells"
    );
}

#[test]
fn the_panel_leaves_a_clear_cell_box_for_the_pixel_previews() {
    let mut snap = GameState::new(1).snapshot();
    snap.hold = Some(PieceKind::T);
    snap.next_queue = [
        PieceKind::O,
        PieceKind::I,
        PieceKind::S,
        PieceKind::Z,
        PieceKind::L,
    ];

    let view = GameView::default();
    let viewport = Viewport::new(60, 40);
    let layout = view.panel_layout(&snap, viewport).expect("panel fits");
    let fb = view.render(&snap, viewport);

    // The reserved box must be blank so the image is never drawn over text.
    let ((hold_cols, hold_rows), _) = view.preview_cells();
    for y in layout.hold.1..layout.hold.1 + hold_rows {
        for x in layout.hold.0..layout.hold.0 + hold_cols {
            assert_eq!(fb.get(x, y).unwrap().ch, ' ', "hold pixel at ({x},{y})");
        }
    }
    for y in layout.next.1..layout.next.1 + layout.next_rows {
        for x in layout.next.0..layout.next.0 + hold_cols {
            assert_eq!(fb.get(x, y).unwrap().ch, ' ', "next pixel at ({x},{y})");
        }
    }

    // Labels sit above their reserved boxes and share the same column.
    let hold_label = fb.get(layout.hold.0, layout.hold.1 - 1).unwrap();
    let next_label = fb.get(layout.next.0, layout.next.1 - 1).unwrap();
    let dump = dump(&fb);
    assert!(dump.contains("HOLD"), "HOLD label");
    assert!(dump.contains("NEXT"), "NEXT label");
    assert!(hold_label.ch != ' ', "HOLD label is above the preview");
    assert!(next_label.ch != ' ', "NEXT label is above the preview");
}

#[test]
fn the_panel_keeps_the_cell_renderer_layout() {
    // SCORE label and value on separate rows, blank separator, LEVEL/LINES, then
    // HOLD and NEXT underneath - the same column the cell renderer drew.
    let mut snap = GameState::new(1).snapshot();
    snap.score = 478;
    snap.level = 1;
    snap.lines = 12;
    let view = GameView::default().with_preview_cells((6, 2), (6, 9));
    let viewport = Viewport::new(80, 24);
    let layout = view.panel_layout(&snap, viewport).expect("panel fits");
    let fb = view.render(&snap, viewport);
    let x = layout.x;
    let top = layout.top;

    let row_text = |y: u16, len: u16| {
        (0..len)
            .map(|dx| fb.get(x + dx, y).unwrap().ch)
            .collect::<String>()
    };
    assert!(row_text(top, 5).starts_with("SCORE"), "SCORE label");
    assert_eq!(
        fb.get(x, top + 1).unwrap().ch,
        '4',
        "the score value sits on its own row"
    );
    assert!(
        row_text(top + 2, 5).trim().is_empty(),
        "blank separator after the score"
    );
    assert!(row_text(top + 3, 5).starts_with("LEVEL"));
    assert!(row_text(top + 4, 5).starts_with("LINES"));
    assert_eq!(
        fb.get(x, layout.hold.1 - 1).unwrap().ch,
        'H',
        "HOLD label above its preview"
    );
    assert_eq!(
        fb.get(x, layout.next.1 - 1).unwrap().ch,
        'N',
        "NEXT label above the strip"
    );
}

#[test]
fn a_terminal_too_narrow_for_the_panel_reports_no_layout() {
    let snap = GameState::new(1).snapshot();
    let view = GameView::default().with_preview_cells((6, 2), (6, 9));
    // The 22-column frame needs 12 more columns for the panel plus its margin,
    // which puts the boundary at 50 columns for this geometry.
    let first = (20..80)
        .find(|width| {
            view.panel_layout(&snap, Viewport::new(*width, 24))
                .is_some()
        })
        .expect("some width fits the panel");
    assert_eq!(
        view.panel_min_width(Viewport::new(first, 24)),
        first,
        "the warning threshold agrees with the layout"
    );
    assert!(first < 60, "the boundary is close to the frame width");
    assert!(
        view.panel_layout(&snap, Viewport::new(48, 24)).is_none(),
        "narrow terminals drop the panel"
    );
}

#[test]
fn an_empty_hold_slot_shows_a_dash() {
    let mut snap = GameState::new(1).snapshot();
    snap.hold = None;
    let view = GameView::default();
    let viewport = Viewport::new(60, 40);
    let layout = view.panel_layout(&snap, viewport).expect("panel fits");
    let fb = view.render(&snap, viewport);
    assert_eq!(fb.get(layout.hold.0, layout.hold.1).unwrap().ch, '-');
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
fn the_board_is_centred_by_default_and_can_anchor_to_the_top() {
    let snap = GameState::new(1).snapshot();
    let viewport = Viewport::new(22, 30);
    let centred = GameView::default().render(&snap, viewport);
    assert_eq!(centred.get(0, 4).unwrap().ch, '┌', "centred vertically");
    let top = GameView::default()
        .with_anchor_y(AnchorY::Top)
        .render(&snap, viewport);
    assert_eq!(top.get(0, 0).unwrap().ch, '┌', "anchored to the top");
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

    // The five previews are pixel layers, so the cell grid stays clear for all
    // three render paths and the reserved strip is identical.
    for (label, fb) in [
        ("local", &without),
        ("adapter", &with_adapter),
        ("observe", &with_observe),
    ] {
        assert_eq!(panel_minos(fb), 0, "{label}: previews are not cells");
        assert!(dump(fb).contains("NEXT"), "{label}: NEXT label");
    }
    let layout = view.panel_layout(&snap, vp).expect("panel fits");
    assert_eq!(layout.next_rows, 7, "the default reservation");
    assert!(layout.hold.1 < layout.next.1, "hold is above next");
}

#[test]
fn a_taller_preview_reservation_keeps_the_panel_inside_the_frame() {
    // The app reserves 12px preview minos, which needs 9 rows for the NEXT strip
    // on an 8x17 cell. The previews are bottom-anchored so that combo/B2B rows
    // never move them.
    let snap = GameState::new(1).snapshot();
    let view = GameView::default().with_preview_cells((6, 2), (6, 9));
    let vp = Viewport::new(60, 40);
    let layout = view.panel_layout(&snap, vp).expect("panel fits");
    let frame_h = view.well_cells().1 + 2;
    assert_eq!(layout.next_rows, 9);
    assert_eq!(
        layout.next.1 + layout.next_rows,
        layout.top + frame_h,
        "the strip ends exactly on the frame's last row"
    );
    assert_eq!(
        layout.hold.1 + 2,
        layout.next.1 - 1,
        "one label row between"
    );

    // combo/B2B must not shift the anchors.
    let mut scored = snap;
    scored.combo = 3;
    scored.back_to_back = true;
    assert_eq!(
        view.panel_layout(&scored, vp).map(|l| (l.hold, l.next)),
        Some((layout.hold, layout.next))
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
fn the_hud_shows_the_guideline_level_and_only_live_chain_status() {
    let mut snap = GameState::new(1).snapshot();
    snap.level = 2;
    snap.lines = 24;
    snap.score = 800;
    snap.combo = 3;
    snap.back_to_back = true;

    let all = dump(&GameView::default().render(&snap, Viewport::new(60, 24)));
    assert!(all.contains("LEVEL 3"), "guideline level (engine+1)");
    assert!(all.contains("COMBO"));
    assert!(all.contains("B2B"));

    let mut idle = snap;
    idle.combo = -1;
    idle.back_to_back = false;
    let all = dump(&GameView::default().render(&idle, Viewport::new(60, 24)));
    assert!(!all.contains("COMBO"), "idle combo is hidden");
    assert!(!all.contains("B2B"), "idle b2b is hidden");
}

#[test]
fn pause_and_game_over_banners_sit_over_the_well() {
    let viewport = Viewport::new(60, 24);
    let view = GameView::default();

    let mut paused = GameState::new(1).snapshot();
    paused.paused = true;
    let text = dump(&view.render(&paused, viewport));
    assert!(text.contains("PAUSED"));
    assert!(text.contains("resume"));

    let mut over = GameState::new(1).snapshot();
    over.game_over = true;
    over.score = 4242;
    let text = dump(&view.render(&over, viewport));
    assert!(text.contains("GAME OVER"));
    assert!(text.contains("4242"), "the final score is shown");
    assert!(text.contains("retry"));
}

#[test]
fn well_origin_is_the_cell_inside_the_border() {
    for (cell_w, cell_h) in [(2u16, 1u16), (1, 1), (3, 1)] {
        let frame = (10 * cell_w + 2, 20 * cell_h + 2);
        for viewport in [
            Viewport::new(frame.0, frame.1),
            Viewport::new(frame.0 + 20, frame.1 + 4),
        ] {
            for anchor in [AnchorY::Center, AnchorY::Top] {
                let view = GameView::new(cell_w, cell_h).with_anchor_y(anchor);
                let fb = view.render(&GameState::new(1).snapshot(), viewport);
                let (ox, oy) = view.well_origin(viewport);
                assert!(
                    matches!(fb.get(ox - 1, oy - 1).unwrap().ch, '┌' | '│' | '─'),
                    "well origin {ox},{oy} must sit inside the frame for {cell_w}x{cell_h}"
                );
                // The origin plus the well's cell box ends on the far border.
                let (cols, rows) = (10 * cell_w, 20 * cell_h);
                assert_eq!(fb.get(ox + cols, oy).unwrap().ch, '│');
                assert_eq!(fb.get(ox, oy + rows).unwrap().ch, '─');
            }
        }
    }
}

#[test]
fn the_well_is_never_drawn_as_character_cells() {
    let mut snap = GameState::new(1).snapshot();
    snap.board[19][0] = 1;
    snap.board[19][1] = 1;
    snap.active = None;
    snap.ghost_y = None;
    let viewport = Viewport::new(60, 24);
    let view = GameView::default();
    let fb = view.render(&snap, viewport);
    let (ox, oy) = view.well_origin(viewport);

    // Minos are pixels now: the whole interior stays background.
    for y in 0..20 {
        for x in 0..20 {
            let cell = fb.get(ox + x, oy + y).unwrap();
            assert_eq!(cell.ch, ' ');
            assert_eq!(cell.style.bg, Rgb::new(28, 28, 28));
        }
    }
    assert_eq!(fb.get(ox - 1, oy - 1).unwrap().ch, '┌');
    assert_eq!(
        panel_minos(&fb),
        0,
        "HOLD/NEXT previews are pixel layers, not cells"
    );
    assert!(dump(&fb).contains("move rotate drop"));
}

#[test]
fn the_well_still_gets_a_pause_and_game_over_banner() {
    let viewport = Viewport::new(60, 24);
    let view = GameView::default();

    let mut paused = GameState::new(1).snapshot();
    paused.paused = true;
    assert!(dump(&view.render(&paused, viewport)).contains("PAUSED"));

    let mut over = GameState::new(1).snapshot();
    over.game_over = true;
    assert!(dump(&view.render(&over, viewport)).contains("GAME OVER"));
}
