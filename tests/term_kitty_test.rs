//! Kitty graphics backend tests: layered well rendering and the wire format.
//!
//! These tests pin the escape encoding, the tmux passthrough envelope, the two
//! well layers (opaque board + transparent motion), and the retransmission
//! policy. They never touch a real terminal.

use std::collections::HashMap;

use tetris_core::core::{ActiveSnapshot, GameSnapshot};
use tetris_core::types::{BOARD_HEIGHT, BOARD_WIDTH, PieceKind, Rotation};
use tetris_terminal::term::Rgb as FbRgb;
use tetris_terminal::term::kitty::{
    BASE64_CHUNK, GraphicsChoice, KittyEscapeWriter, KittySupport, PREVIEW_COLS, PixelCanvas,
    PixelFormat, PreviewRenderer, Rgb, WELL_BG, WellGeometry, WellRenderer, board_hash, detect,
    motion_hash,
};

fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |key: &str| map.get(key).cloned()
}

fn empty_env() -> impl Fn(&str) -> Option<String> {
    |_: &str| None
}

const ESC: u8 = 0x1b;

fn apc_spans(bytes: &[u8]) -> Vec<&[u8]> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == ESC && bytes[i + 1] == ESC {
            // Doubled escape: this is a tmux passthrough payload, not an APC.
            i += 2;
            continue;
        }
        if bytes[i] == ESC && bytes[i + 1] == b'_' && bytes[i + 2] == b'G' {
            let mut j = i + 3;
            while j + 1 < bytes.len() {
                if bytes[j] == ESC && bytes[j + 1] == b'\\' {
                    spans.push(&bytes[i..j + 2]);
                    i = j + 2;
                    break;
                }
                j += 1;
            }
            if j + 1 >= bytes.len() {
                break;
            }
        } else {
            i += 1;
        }
    }
    spans
}

fn control_and_payload(apc: &[u8]) -> (String, &[u8]) {
    let body = &apc[3..apc.len() - 2];
    match body.iter().position(|b| *b == b';') {
        Some(split) => (
            String::from_utf8(body[..split].to_vec()).unwrap(),
            &body[split + 1..],
        ),
        None => (String::from_utf8(body.to_vec()).unwrap(), &[][..]),
    }
}

fn control_values(control: &str) -> HashMap<&str, &str> {
    control
        .split(',')
        .filter_map(|part| part.split_once('='))
        .collect()
}

fn base64_decode(text: &[u8]) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &b in text {
        if b == b'=' || b == b'\n' {
            continue;
        }
        let v = TABLE.iter().position(|c| *c == b).expect("base64 alphabet") as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}

fn inflate(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .expect("zlib payload");
    out
}

fn support() -> KittySupport {
    KittySupport {
        tmux_passthrough: false,
    }
}

fn geometry() -> WellGeometry {
    // 2x1 terminal cells per mino, 8x17 pixel cells: a 160x340 device box.
    WellGeometry::new(2, 1, (8, 17))
}

fn sample_canvas() -> PixelCanvas {
    let (w, h) = geometry().pixels();
    let mut canvas = PixelCanvas::new(w, h, PixelFormat::Rgb);
    canvas.fill(Rgb::new(28, 28, 28));
    canvas.fill_rect(0, 0, 16, 16, Rgb::new(79, 209, 196));
    canvas
}

fn board_with(cells: &[(usize, usize, u8)]) -> GameSnapshot {
    let mut snap = GameSnapshot::default();
    for &(x, y, value) in cells {
        snap.board[y][x] = value;
    }
    snap
}

/// Centre of the square mino drawn in board cell `(x, y)`.
fn mino_center(geom: WellGeometry, x: u16, y: u16) -> (u32, u32) {
    let (mx, my) = geom.mino_origin(x, y);
    let side = geom.mino_side();
    (mx + side / 2, my + side / 2)
}

fn started_snapshot() -> GameSnapshot {
    let mut state = tetris_core::core::GameState::new(1);
    state.start();
    state.snapshot()
}

// ---------------------------------------------------------------- detection

#[test]
fn explicit_off_override_wins_over_every_terminal_marker() {
    let choice = detect(&env_from(&[
        ("TUI_TETRIS_GRAPHICS", "off"),
        ("TERM_PROGRAM", "ghostty"),
        ("TERM_PROGRAM_VERSION", "1.4.0"),
    ]));
    assert_eq!(choice, GraphicsChoice::Off);
}

#[test]
fn ghostty_marks_enable_kitty_graphics() {
    assert_eq!(
        detect(&env_from(&[
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.4.0"),
        ])),
        GraphicsChoice::Kitty(support())
    );
    // Ghostty exports these inside multiplexers as well.
    assert_eq!(
        detect(&env_from(&[(
            "GHOSTTY_RESOURCES_DIR",
            "/Applications/Ghostty.app/Contents/Resources/ghostty",
        )])),
        GraphicsChoice::Kitty(support())
    );
}

#[test]
fn kitty_and_wezterm_and_rio_are_recognised() {
    for pairs in [
        vec![("TERM", "xterm-kitty")],
        vec![("KITTY_WINDOW_ID", "1")],
        vec![("TERM_PROGRAM", "WezTerm")],
        vec![("TERM_PROGRAM", "rio")],
    ] {
        let choice = detect(&env_from(&pairs));
        assert!(
            choice.is_kitty(),
            "env {pairs:?} should select kitty graphics"
        );
    }
}

#[test]
fn unknown_and_missing_terminals_stay_off() {
    assert_eq!(detect(&empty_env()), GraphicsChoice::Off);
    assert_eq!(
        detect(&env_from(&[("TERM", "xterm-256color")])),
        GraphicsChoice::Off
    );
    assert_eq!(detect(&env_from(&[("TERM", "dumb")])), GraphicsChoice::Off);
}

#[test]
fn tmux_requires_the_passthrough_envelope() {
    assert_eq!(
        detect(&env_from(&[
            ("TERM_PROGRAM", "ghostty"),
            ("TERM", "screen-256color"),
            ("TMUX", "/tmp/tmux-501/default,4242,0"),
        ])),
        GraphicsChoice::Kitty(KittySupport {
            tmux_passthrough: true,
        })
    );
}

#[test]
fn forcing_kitty_inside_an_unknown_multiplexer_keeps_passthrough_on() {
    assert_eq!(
        detect(&env_from(&[
            ("TUI_TETRIS_GRAPHICS", "kitty"),
            ("TMUX", "/tmp/tmux-501/default,4240,0"),
        ])),
        GraphicsChoice::Kitty(KittySupport {
            tmux_passthrough: true,
        })
    );
}

// ------------------------------------------------------------------ encoder

#[test]
fn transmit_places_a_layer_with_scale_and_z_index() {
    let canvas = sample_canvas();
    let mut writer = KittyEscapeWriter::new(support());
    let mut out = Vec::new();

    writer
        .transmit_and_place(&canvas, 7, 3, 1, geometry(), (4, 5), &mut out)
        .unwrap();

    let spans = apc_spans(&out);
    assert_eq!(spans.len(), 1, "small payload must be a single APC");
    let (control, payload) = control_and_payload(spans[0]);
    let values = control_values(&control);
    assert_eq!(values.get("a"), Some(&"T"));
    assert_eq!(values.get("f"), Some(&"24"), "board layer is packed RGB");
    assert_eq!(values.get("o"), Some(&"z"));
    assert_eq!(values.get("i"), Some(&"7"));
    assert_eq!(values.get("p"), Some(&"3"));
    assert_eq!(values.get("c"), Some(&"20"), "well is 10 minos x 2 cells");
    assert_eq!(values.get("r"), Some(&"20"));
    assert_eq!(values.get("C"), Some(&"1"), "do not move the cursor");
    assert_eq!(
        values.get("q"),
        Some(&"2"),
        "no response traffic on the tty"
    );
    assert_eq!(values.get("z"), Some(&"1"), "layer stacking");
    assert_eq!(values.get("s"), Some(&"160"));
    assert_eq!(values.get("v"), Some(&"340"));
    assert!(!payload.is_empty());
    // The image lands on the cell the caller asked for.
    assert!(out.starts_with(b"\x1b[6;5H"), "cursor move first");
}

#[test]
fn rgba_layers_declare_the_32_bit_format() {
    let (w, h) = geometry().pixels();
    let mut canvas = PixelCanvas::new(w, h, PixelFormat::Rgba);
    canvas.fill_rect(0, 0, 8, 8, Rgb::new(1, 2, 3));
    let mut writer = KittyEscapeWriter::new(support());
    let mut out = Vec::new();

    writer
        .transmit_and_place(&canvas, 1, 1, 1, geometry(), (0, 0), &mut out)
        .unwrap();

    let (control, payload) = control_and_payload(apc_spans(&out)[0]);
    assert_eq!(control_values(&control).get("f"), Some(&"32"));
    assert_eq!(
        inflate(&base64_decode(payload)),
        canvas.data(),
        "RGBA bytes travel unchanged"
    );
}

#[test]
fn payload_round_trips_through_base64_and_zlib() {
    let canvas = sample_canvas();
    let mut writer = KittyEscapeWriter::new(support());
    let mut out = Vec::new();
    writer
        .transmit_and_place(&canvas, 1, 1, 0, geometry(), (0, 0), &mut out)
        .unwrap();

    let mut encoded = Vec::new();
    for span in apc_spans(&out) {
        let (_, payload) = control_and_payload(span);
        encoded.extend_from_slice(payload);
    }

    assert_eq!(inflate(&base64_decode(&encoded)), canvas.data());
}

#[test]
fn large_payloads_chunk_on_base64_quartets_and_repeat_quiet_mode() {
    let geom = WellGeometry::new(2, 1, (20, 24));
    let (w, h) = geom.pixels();
    let mut canvas = PixelCanvas::new(w, h, PixelFormat::Rgb);
    // Noise defeats compression so the encoded stream needs many chunks.
    let mut state: u32 = 12345;
    for byte in canvas.data_mut() {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *byte = (state >> 24) as u8;
    }

    let mut writer = KittyEscapeWriter::new(support());
    let mut out = Vec::new();
    writer
        .transmit_and_place(&canvas, 1, 1, 0, geom, (0, 0), &mut out)
        .unwrap();

    let spans = apc_spans(&out);
    assert!(spans.len() > 2, "expected chunking, got {}", spans.len());
    for (index, span) in spans.iter().enumerate() {
        let (control, payload) = control_and_payload(span);
        assert!(
            payload.len() <= BASE64_CHUNK,
            "chunk {index} is {} bytes",
            payload.len()
        );
        assert_eq!(payload.len() % 4, 0, "chunk {index} splits a quartet");
        assert!(
            control.contains("q=2"),
            "chunk {index} must stay quiet: {control}"
        );
        let expected_m = if index + 1 == spans.len() {
            "m=0"
        } else {
            "m=1"
        };
        assert!(control.contains(expected_m), "chunk {index}: {control}");
        if index > 0 {
            assert!(
                !control.contains("a=T"),
                "continuation {index} repeats control data: {control}"
            );
            assert!(
                !control.contains("i=1"),
                "continuation {index} repeats the image id: {control}"
            );
        }
    }
}

#[test]
fn delete_all_is_quiet_and_frees_terminal_storage() {
    let mut writer = KittyEscapeWriter::new(support());
    let mut out = Vec::new();

    writer.delete_all(&mut out);

    let spans = apc_spans(&out);
    assert_eq!(spans.len(), 1);
    let (control, payload) = control_and_payload(spans[0]);
    assert_eq!(control, "a=d,d=A,q=2");
    assert!(payload.is_empty());
}

#[test]
fn tmux_passthrough_doubles_every_escape_and_wraps_each_chunk() {
    let canvas = sample_canvas();
    let mut writer = KittyEscapeWriter::new(KittySupport {
        tmux_passthrough: true,
    });
    let mut out = Vec::new();

    writer
        .transmit_and_place(&canvas, 1, 1, 0, geometry(), (0, 0), &mut out)
        .unwrap();
    writer.delete_all(&mut out);

    assert!(apc_spans(&out).is_empty(), "every APC must be wrapped");

    let prefix = b"\x1bPtmux;";
    let mut envelopes = 0;
    let mut cursor = 0;
    while let Some(found) = out[cursor..]
        .windows(prefix.len())
        .position(|w| w == prefix)
    {
        let mut i = cursor + found + prefix.len();
        loop {
            assert!(i + 1 < out.len(), "unterminated passthrough envelope");
            if out[i] == ESC {
                if out[i + 1] == ESC {
                    i += 2;
                    continue;
                }
                assert_eq!(out[i + 1], b'\\', "envelope must end with ST");
                cursor = i + 2;
                envelopes += 1;
                break;
            }
            i += 1;
        }
    }
    assert_eq!(envelopes, 2, "one DCS envelope per graphics command");
}

// ---------------------------------------------------------------- geometry

#[test]
fn geometry_maps_minos_onto_a_square_grid() {
    let geom = geometry();
    assert_eq!(geom.cells(), (BOARD_WIDTH as u16 * 2, BOARD_HEIGHT as u16));
    assert_eq!(geom.pixels(), (160, 340));
    assert_eq!(geom.band_pixels(), (16, 17), "bands tile the well box");
    assert_eq!(geom.mino_side(), 16, "largest square that fits a band");
    assert_eq!(geom.grid_origin(), (0, 0), "bands leave no leftover here");
}

#[test]
fn the_grid_fills_the_well_box_so_rows_touch_the_frame() {
    // Whatever the cell aspect, the bands must span the whole layer; otherwise
    // the bottom row floats above the frame.
    for (cell_w, cell_h) in [(2u16, 1u16), (1, 1), (3, 1), (2, 2)] {
        for cell_px in [(8u16, 17u16), (8, 16), (9, 19), (10, 21)] {
            let geom = WellGeometry::new(cell_w, cell_h, cell_px).with_layer_pixels(
                10 * u32::from(cell_w) * u32::from(cell_px.0),
                20 * u32::from(cell_h) * u32::from(cell_px.1),
            );
            let (band_w, band_h) = geom.band_pixels();
            let (ox, oy) = geom.grid_origin();
            let (layer_w, layer_h) = geom.pixels();
            assert!(
                layer_w - (ox + band_w * 10) < band_w,
                "{cell_w}x{cell_h} {cell_px:?}"
            );
            assert!(
                layer_h - (oy + band_h * 20) < band_h,
                "{cell_w}x{cell_h} {cell_px:?}"
            );

            // The bottom-right mino's last pixel row is inside the frame.
            let side = geom.mino_side();
            let (mx, my) = geom.mino_origin(9, 19);
            assert!(mx + side <= layer_w);
            assert!(my + side <= layer_h);
            assert!(
                layer_h - (my + side) <= band_h,
                "bottom row must touch the frame"
            );
        }
    }
}

#[test]
fn measured_layer_pixels_replace_the_integer_box() {
    let geom = geometry().with_layer_pixels(170, 344);
    assert_eq!(geom.pixels(), (170, 344));
    assert_eq!(geom.mino_side(), 17);
}

// ------------------------------------------------------------- board layer

#[test]
fn board_layer_paints_settled_minos_as_opaque_blocks() {
    let snap = board_with(&[(0, 0, 1), (9, 19, 3)]);
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();

    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    let canvas = well.board();
    let (x, y) = mino_center(geom, 0, 0);
    assert_eq!(
        canvas.pixel(x, y),
        Some(FbRgb::new(79, 209, 196)),
        "mino interior uses the piece colour"
    );
    let (x, y) = mino_center(geom, 9, 19);
    assert_eq!(
        canvas.pixel(x, y),
        Some(FbRgb::new(158, 122, 235)),
        "bottom-right mino"
    );
    let (x, y) = mino_center(geom, 5, 5);
    let empty = canvas.pixel(x, y).expect("empty cell");
    assert!(
        empty.r < 60 && empty.g < 60 && empty.b < 60,
        "empty cells keep the dark well backdrop, got {empty:?}"
    );
    assert_eq!(canvas.format(), PixelFormat::Rgb, "board is opaque");
}

#[test]
fn neighbouring_cells_never_expose_a_background_gap() {
    // Two settled cells side by side and stacked: the seam must be made of
    // bevel pixels only. Background showing between cells is what made pieces
    // look like loose tiles.
    let snap = board_with(&[(0, 17, 2), (1, 17, 2), (0, 18, 2), (1, 18, 2)]);
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    let canvas = well.board();
    let side = geom.mino_side();
    assert!(side >= 4);
    let (mx, my) = geom.mino_origin(0, 0);
    let probe_row = my + 17 * 17 + side / 2;
    for x in mx..mx + side {
        assert_ne!(
            canvas.pixel(x, probe_row),
            Some(WELL_BG),
            "background pixel at x={x} on the horizontal seam"
        );
    }
    let probe_col = mx + side / 2;
    for y in my + 17 * 17..my + 18 * 17 + side {
        assert_ne!(
            canvas.pixel(probe_col, y),
            Some(WELL_BG),
            "background pixel at y={y} on the vertical seam"
        );
    }
}

#[test]
fn a_locked_tetromino_looks_exactly_like_the_falling_one() {
    // The whole point of the region painter: settling must not restyle the
    // piece. Render it as the active piece, then as four settled cells, and
    // require byte-identical pixels for those cells.
    let geom = geometry();
    let falling = GameSnapshot {
        active: Some(ActiveSnapshot {
            kind: PieceKind::O,
            rotation: Rotation::North,
            x: 4,
            y: 8,
        }),
        ..GameSnapshot::default()
    };
    let locked = board_with(&[(5, 8, 2), (6, 8, 2), (5, 9, 2), (6, 9, 2)]);

    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&falling, geom, (0, 0), &mut out).unwrap();
    let motion = well.motion().clone();
    let mut out = Vec::new();
    well.sync(&locked, geom, (0, 0), &mut out).unwrap();
    let board = well.board();

    for &(bx, by) in &[(5u16, 8u16), (6, 8), (5, 9), (6, 9)] {
        let (mx, my) = geom.mino_origin(bx, by);
        for y in my..my + geom.band_pixels().1 {
            for x in mx..mx + geom.band_pixels().0 {
                if motion.alpha(x, y) != Some(255) {
                    continue;
                }
                assert_eq!(
                    motion.pixel(x, y),
                    board.pixel(x, y),
                    "locked cell pixel ({x},{y}) differs from the falling piece"
                );
            }
        }
    }
}

#[test]
fn a_tetromino_is_drawn_as_one_solid_shape() {
    // The O piece is 2x2 cells: everything inside its outline must be body
    // colour, with no internal cell borders.
    let snap = GameSnapshot {
        active: Some(ActiveSnapshot {
            kind: PieceKind::O,
            rotation: Rotation::North,
            x: 4,
            y: 8,
        }),
        ..GameSnapshot::default()
    };
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    let motion = well.motion();
    let side = geom.mino_side();
    let body = Rgb::new(245, 224, 94);
    // O occupies (x+1, y) and (x+1, y+1) and (x+2, ...).
    let (mx, my) = geom.mino_origin(5, 8);
    for y in my + 1..my + 2 * side - 1 {
        for x in mx + 1..mx + 2 * side - 1 {
            assert_eq!(
                motion.pixel(x, y),
                Some(body),
                "interior pixel ({x},{y}) must be body colour, not a seam"
            );
        }
    }
}

#[test]
fn unchanged_board_does_not_retransmit() {
    let snap = board_with(&[(0, 0, 1)]);
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();

    assert!(well.sync(&snap, geom, (0, 0), &mut out).unwrap());
    assert!(!out.is_empty());

    out.clear();
    assert!(!well.sync(&snap, geom, (0, 0), &mut out).unwrap());
    assert!(out.is_empty(), "identical frames must not touch the wire");
}

// ------------------------------------------------------------ motion layer

#[test]
fn motion_layer_is_transparent_except_the_piece_and_ghost() {
    let snap = GameSnapshot {
        board: [[1; BOARD_WIDTH as usize]; BOARD_HEIGHT as usize],
        active: Some(ActiveSnapshot {
            kind: PieceKind::O,
            rotation: Rotation::North,
            x: 4,
            y: 2,
        }),
        ghost_y: Some(10),
        ..GameSnapshot::default()
    };
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    let motion = well.motion();
    assert_eq!(motion.format(), PixelFormat::Rgba);
    let (x, y) = mino_center(geom, 3, 3);
    assert_eq!(
        motion.alpha(x, y),
        Some(0),
        "empty area is fully transparent so the board shows through"
    );
    // The O shape is offset by (+1, 0) from its origin cell.
    let (x, y) = mino_center(geom, 5, 2);
    let piece = motion.pixel(x, y).expect("piece pixel");
    assert_eq!(piece, Rgb::new(245, 224, 94));

    // The ghost is a translucent silhouette: visible, but clearly weaker than
    // the piece, with a denser rim.
    let side = geom.mino_side();
    let (gx, gy) = geom.mino_origin(5, 10);
    let fill = motion
        .alpha(gx + side / 2, gy + side / 2)
        .expect("ghost fill");
    assert!(fill > 0, "the ghost must be visible");
    assert!(fill < 128, "the ghost must stay translucent, got {fill}");
    let rim = motion.alpha(gx, gy).expect("ghost rim");
    assert!(
        rim > fill,
        "the rim is denser than the fill: {rim} vs {fill}"
    );
}

#[test]
fn motion_layer_has_a_drop_shadow_below_the_piece() {
    let snap = GameSnapshot {
        active: Some(ActiveSnapshot {
            kind: PieceKind::I,
            rotation: Rotation::North,
            x: 3,
            y: 5,
        }),
        ..GameSnapshot::default()
    };
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    let motion = well.motion();
    let side = geom.mino_side();
    // I(North) occupies (dx, 1), so the piece covers row 6. The shadow is offset
    // by (+2, +2): sample the sliver that sticks out below the block.
    let (mx, my) = geom.mino_origin(3, 6);
    let x = mx + side / 2;
    let y = my + side + 1;
    let alpha = motion.alpha(x, y).expect("shadow pixel");
    assert!(alpha > 0, "shadow must be visible, got alpha {alpha}");
    assert!(
        alpha < 255,
        "shadow is translucent so the board shows through"
    );
}

#[test]
fn motion_layer_ignores_board_changes() {
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let snap = started_snapshot();
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    // A locked cell changes the board layer only.
    let mut locked = snap;
    locked.board[19][0] = 1;
    out.clear();
    assert!(well.sync(&locked, geom, (0, 0), &mut out).unwrap());
    let spans = apc_spans(&out);
    assert_eq!(spans.len(), 1, "only the board layer is retransmitted");
    let (control, _) = control_and_payload(spans[0]);
    assert_eq!(control_values(&control).get("z"), Some(&"0"));
}

#[test]
fn moving_the_piece_retransmits_only_the_motion_layer() {
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut snap = board_with(&[(0, 19, 1)]);
    snap.active = Some(ActiveSnapshot {
        kind: PieceKind::T,
        rotation: Rotation::North,
        x: 3,
        y: 4,
    });
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    snap.active = Some(ActiveSnapshot {
        y: 5,
        ..snap.active.unwrap()
    });
    out.clear();
    assert!(well.sync(&snap, geom, (0, 0), &mut out).unwrap());

    let spans = apc_spans(&out);
    assert_eq!(spans.len(), 1);
    let (control, _) = control_and_payload(spans[0]);
    let values = control_values(&control);
    assert_eq!(values.get("z"), Some(&"1"), "motion layer is on top");
    assert_eq!(values.get("f"), Some(&"32"));
}

#[test]
fn line_clear_flashes_and_pause_dims_the_motion_layer() {
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut full_row = [(0usize, 19usize, 1u8); BOARD_WIDTH as usize];
    for (x, cell) in full_row.iter_mut().enumerate() {
        *cell = (x, 19, (x % 7 + 1) as u8);
    }
    let mut snap = board_with(&full_row);
    snap.active = None;
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    snap.timers.line_clear_ms = 120;
    out.clear();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();
    let (x, y) = mino_center(geom, 0, 19);
    let flashed = well.motion().alpha(x, y).expect("flash pixel");
    assert!(flashed > 0, "the cleared row must flash");

    snap.timers.line_clear_ms = 0;
    snap.paused = true;
    out.clear();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();
    let (x, y) = mino_center(geom, 5, 5);
    let dim = well.motion().alpha(x, y).expect("dim pixel");
    assert!(dim > 0, "pausing dims the whole well");
}

// ----------------------------------------------------------------- hashes

#[test]
fn hashes_track_only_what_their_layer_draws() {
    let mut snap = GameSnapshot::default();
    let board = board_hash(&snap);
    let motion = motion_hash(&snap, geometry());

    snap.score = 999;
    snap.level = 9;
    snap.next_queue[0] = PieceKind::S;
    assert_eq!(board_hash(&snap), board, "HUD fields are not board pixels");
    assert_eq!(
        motion_hash(&snap, geometry()),
        motion,
        "HUD fields are not motion pixels"
    );

    snap.board[0][0] = 1;
    assert_ne!(board_hash(&snap), board);
    assert_eq!(
        motion_hash(&snap, geometry()),
        motion,
        "the settled board belongs to the board layer"
    );

    let moved = GameSnapshot {
        active: Some(ActiveSnapshot {
            kind: PieceKind::T,
            rotation: Rotation::North,
            x: 3,
            y: 0,
        }),
        ..GameSnapshot::default()
    };
    assert_ne!(motion_hash(&moved, geometry()), motion);
    assert_eq!(
        board_hash(&moved),
        board_hash(&GameSnapshot::default()),
        "the falling piece belongs to the motion layer"
    );
}

// ---------------------------------------------------------------- lifecycle

#[test]
fn geometry_changes_resize_and_retransmit_both_layers() {
    let snap = started_snapshot();
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    let resized = WellGeometry::new(1, 1, (12, 12));
    out.clear();
    assert!(well.sync(&snap, resized, (0, 0), &mut out).unwrap());

    assert_eq!(spans_of(&out), 2, "board and motion layers both resend");
    assert_eq!(well.board().width(), resized.pixels().0);
    assert_eq!(well.motion().height(), resized.pixels().1);
    assert_eq!(well.geometry(), resized);
}

fn spans_of(out: &[u8]) -> usize {
    apc_spans(out).len()
}

#[test]
fn invalidate_forces_a_full_retransmission() {
    let snap = board_with(&[(0, 0, 1)]);
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    out.clear();
    well.invalidate();
    assert!(well.sync(&snap, geom, (0, 0), &mut out).unwrap());
    assert_eq!(spans_of(&out), 2, "a cleared screen needs both layers");
}

#[test]
fn shutdown_removes_every_placement_once() {
    let snap = board_with(&[(0, 0, 1)]);
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    out.clear();
    well.shutdown(&mut out);
    let spans = apc_spans(&out);
    assert_eq!(spans.len(), 1);
    let (control, _) = control_and_payload(spans[0]);
    assert_eq!(control, "a=d,d=A,q=2");

    out.clear();
    well.shutdown(&mut out);
    assert!(out.is_empty(), "shutdown is idempotent");
}

#[test]
fn shutdown_still_deletes_after_an_invalidate() {
    let snap = board_with(&[(0, 0, 1)]);
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    // invalidate() clears the per-layer markers but the terminal still holds the
    // images, so quitting must still delete them.
    well.invalidate();
    out.clear();
    well.shutdown(&mut out);
    assert_eq!(spans_of(&out), 1, "delete must not depend on drawn markers");
}

// ----------------------------------------------------------------- previews

#[test]
fn preview_minos_are_exact_squares() {
    // The HUD previews used to be half-block glyphs, whose height came from the
    // font (about 17% too tall). As pixels they are exact.
    let snap = GameSnapshot {
        hold: Some(PieceKind::O),
        ..GameSnapshot::default()
    };
    let mut previews = PreviewRenderer::new(support(), 16);
    let mut out = Vec::new();
    previews.sync(&snap, (0, 0), (0, 0), &mut out).unwrap();

    let canvas = previews.hold();
    let side = previews.mino();
    let opaque: Vec<_> = (0..canvas.height())
        .flat_map(|y| (0..canvas.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| canvas.alpha(x, y) == Some(255))
        .collect();
    let min_x = opaque.iter().map(|(x, _)| *x).min().unwrap();
    let max_x = opaque.iter().map(|(x, _)| *x).max().unwrap();
    let min_y = opaque.iter().map(|(_, y)| *y).min().unwrap();
    let max_y = opaque.iter().map(|(_, y)| *y).max().unwrap();
    assert_eq!(
        (max_x - min_x + 1, max_y - min_y + 1),
        (2 * side, 2 * side),
        "the O preview must be two exact mino squares"
    );
}

#[test]
fn preview_layers_are_native_size_sprites() {
    let snap = GameSnapshot {
        hold: Some(PieceKind::T),
        next_queue: [
            PieceKind::O,
            PieceKind::I,
            PieceKind::S,
            PieceKind::Z,
            PieceKind::L,
        ],
        ..GameSnapshot::default()
    };
    let mut previews = PreviewRenderer::new(support(), 16);
    let mut out = Vec::new();
    previews.sync(&snap, (3, 5), (3, 8), &mut out).unwrap();

    let spans = apc_spans(&out);
    assert_eq!(spans.len(), 2, "one image for HOLD, one for NEXT");
    for span in &spans {
        let (control, payload) = control_and_payload(span);
        let values = control_values(&control);
        assert_eq!(values.get("f"), Some(&"32"), "previews are RGBA");
        assert!(
            !values.contains_key("c"),
            "a native-size sprite must not ask for cell scaling: {control}"
        );
        assert!(!payload.is_empty());
    }
    // The HOLD image lands at its own anchor, the NEXT strip below it.
    assert!(out.starts_with(b"\x1b[6;4H"), "cursor move first");
    // Five slots with a visible gap between the pieces, so they read as separate
    // shapes instead of one column.
    let side = previews.mino();
    assert_eq!(side, 8, "half a 16px board mino, like the cell renderer");
    assert_eq!(
        previews.next().height(),
        5 * (2 * side + 2 * side) - 2 * side,
        "five 2-mino slots with a blank half-slot between pieces"
    );
    assert_eq!(
        previews.next().width(),
        PREVIEW_COLS * side,
        "wide enough for an I piece"
    );
}

#[test]
fn previews_ignore_hud_only_scoring_changes() {
    let snap = GameSnapshot {
        next_queue: [PieceKind::O; 5],
        ..GameSnapshot::default()
    };
    let mut previews = PreviewRenderer::new(support(), 16);
    let mut out = Vec::new();
    previews.sync(&snap, (0, 0), (0, 0), &mut out).unwrap();

    let mut scored = snap;
    scored.score = 1234;
    scored.lines = 12;
    out.clear();
    assert!(!previews.sync(&scored, (0, 0), (0, 0), &mut out).unwrap());
    assert!(out.is_empty(), "score changes are text-only");

    // ...but a new queue retransmits the strip, and a moved anchor re-places it.
    let mut queued = snap;
    queued.next_queue[4] = PieceKind::T;
    out.clear();
    assert!(previews.sync(&queued, (0, 0), (0, 0), &mut out).unwrap());
    out.clear();
    assert!(previews.sync(&queued, (0, 0), (0, 4), &mut out).unwrap());
    assert_eq!(spans_of(&out), 1, "only the re-placed strip is re-sent");
}

#[test]
fn preview_shutdown_removes_the_images() {
    let snap = GameSnapshot {
        hold: Some(PieceKind::I),
        ..GameSnapshot::default()
    };
    let mut previews = PreviewRenderer::new(support(), 16);
    let mut out = Vec::new();
    previews.sync(&snap, (0, 0), (0, 0), &mut out).unwrap();
    out.clear();
    previews.shutdown(&mut out);
    assert_eq!(spans_of(&out), 1);
    out.clear();
    previews.shutdown(&mut out);
    assert!(out.is_empty(), "shutdown is idempotent");
}
