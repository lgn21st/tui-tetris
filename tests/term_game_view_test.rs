use tui_tetris::core::GameState;
use tui_tetris::term::{GameView, Viewport};
use tui_tetris::types::PieceKind;

#[test]
fn term_view_renders_border_corners() {
    let state = GameState::new(1);
    let view = GameView::default();

    // With cell_w=2 and cell_h=1:
    // board pixels = 10*2 by 20*1 => 20x20
    // plus border => 22x22
    let vp = Viewport::new(22, 22);
    let fb = view.render(&state, vp);

    assert_eq!(fb.get(0, 0).unwrap().ch, '┌');
    assert_eq!(fb.get(21, 0).unwrap().ch, '┐');
    assert_eq!(fb.get(0, 21).unwrap().ch, '└');
    assert_eq!(fb.get(21, 21).unwrap().ch, '┘');
}

#[test]
fn term_view_renders_locked_cell_as_two_chars_wide() {
    let mut state = GameState::new(1);
    // Put a locked I block at bottom-left.
    assert!(state.board.set(0, 19, Some(PieceKind::I)));
    state.active = None;

    let view = GameView::default();
    let vp = Viewport::new(22, 22);
    let fb = view.render(&state, vp);

    // Inside border: (1,1) origin. Each cell is 2 chars wide.
    let x0 = 1;
    let y0 = 1 + 19;
    assert_eq!(fb.get(x0, y0).unwrap().ch, '█');
    assert_eq!(fb.get(x0 + 1, y0).unwrap().ch, '█');
}

#[test]
fn term_view_draws_side_panel_when_wide_enough() {
    let mut state = GameState::new(1);
    state.start();
    state.score = 1234;
    state.level = 2;
    state.lines = 10;
    state.hold = Some(PieceKind::T);

    let view = GameView::default();
    // Wider than the 22x22 board frame to allow a panel.
    let fb = view.render(&state, Viewport::new(60, 22));

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
