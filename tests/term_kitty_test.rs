//! Kitty graphics backend tests: wire format, well layers, HUD previews.
//!
//! These never touch a real terminal; `scripts/kitty_smoke_test.py` drives the
//! whole binary through a pseudo-terminal.

use std::collections::HashMap;

use tetris_core::core::{ActiveSnapshot, GameSnapshot};
use tetris_core::types::{BOARD_HEIGHT, BOARD_WIDTH, PieceKind, Rotation};
use tetris_terminal::term::Rgb as FbRgb;
use tetris_terminal::term::kitty::{
    BASE64_CHUNK, KittyEscapeWriter, KittySupport, PREVIEW_COLS, PixelCanvas, PixelFormat,
    PreviewRenderer, Rgb, WELL_BG, WellGeometry, WellRenderer, board_hash, detect, motion_hash,
};

const ESC: u8 = 0x1b;

fn env_from(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |key: &str| map.get(key).cloned()
}

fn support() -> KittySupport {
    KittySupport {
        tmux_passthrough: false,
    }
}

/// A 1x display: 8x17 cells, 20x19 cell box, 160x323 pixels.
fn geometry() -> WellGeometry {
    WellGeometry::new((20, 19), (160, 323))
}

fn board_with(cells: &[(usize, usize, u8)]) -> GameSnapshot {
    let mut snap = GameSnapshot::default();
    for &(x, y, value) in cells {
        snap.board[y][x] = value;
    }
    snap
}

fn started_snapshot() -> GameSnapshot {
    let mut state = tetris_core::core::GameState::new(1);
    state.start();
    state.snapshot()
}

fn mino_center(geom: WellGeometry, x: u16, y: u16) -> (u32, u32) {
    let (x0, y0, w, h) = geom.cell_rect(x, y);
    (x0 + w / 2, y0 + h / 2)
}

// ------------------------------------------------------------------ wire bits

fn apc_spans(bytes: &[u8]) -> Vec<&[u8]> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == ESC && bytes[i + 1] == ESC {
            i += 2; // doubled escape: tmux passthrough payload
            continue;
        }
        if bytes[i] == ESC && bytes[i + 1] == b'_' && bytes[i + 2] == b'G' {
            let Some(end) = bytes[i + 3..]
                .windows(2)
                .position(|w| w == b"\x1b\\")
                .map(|offset| i + 3 + offset)
            else {
                break;
            };
            spans.push(&bytes[i..end + 2]);
            i = end + 2;
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
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in text {
        if b == b'=' {
            continue;
        }
        acc = (acc << 6) | TABLE.iter().position(|c| *c == b).expect("base64") as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
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

/// Decode every chunk of the single transmit in `out`.
fn decoded_image(out: &[u8]) -> (String, Vec<u8>) {
    let spans = apc_spans(out);
    let mut encoded = Vec::new();
    for span in &spans {
        encoded.extend_from_slice(control_and_payload(span).1);
    }
    let control = control_and_payload(spans[0]).0;
    (control, inflate(&base64_decode(&encoded)))
}

// ---------------------------------------------------------------- detection

#[test]
fn terminal_detection_is_strict_about_kitty_support() {
    let cases: Vec<(Vec<(&str, &str)>, bool)> = vec![
        (vec![("TERM_PROGRAM", "ghostty")], true),
        (vec![("GHOSTTY_RESOURCES_DIR", "/ghostty")], true),
        (vec![("TERM", "xterm-kitty")], true),
        (vec![("KITTY_WINDOW_ID", "1")], true),
        (vec![("TERM_PROGRAM", "WezTerm")], true),
        (vec![("TERM_PROGRAM", "rio")], true),
        (vec![("TUI_TETRIS_GRAPHICS", "kitty")], true),
        (vec![("TERM", "xterm-256color")], false),
        (vec![("TERM", "dumb")], false),
        (vec![], false),
    ];
    for (env, expected) in cases {
        assert_eq!(
            detect(&env_from(&env)).is_some(),
            expected,
            "env {env:?} should{} select kitty graphics",
            if expected { "" } else { " not" }
        );
    }
    assert_eq!(
        detect(&env_from(&[("TUI_TETRIS_GRAPHICS", "off")])),
        None,
        "an explicit opt-out wins"
    );
    assert_eq!(
        detect(&env_from(&[
            ("TERM_PROGRAM", "ghostty"),
            ("TMUX", "/tmp/t,1,0")
        ])),
        Some(KittySupport {
            tmux_passthrough: true
        }),
        "tmux needs the passthrough envelope"
    );
}

// ------------------------------------------------------------------ encoder

#[test]
fn transmit_places_one_quiet_chunk_with_the_right_keys() {
    let mut wiring = PixelCanvas::new(160, 323, PixelFormat::Rgb);
    wiring.fill(Rgb::new(28, 28, 28));
    wiring.fill_rect(0, 0, 16, 16, Rgb::new(79, 209, 196));
    let mut writer = KittyEscapeWriter::new(support());
    let mut out = Vec::new();

    writer
        .transmit_and_place(&wiring, 7, 3, 1, geometry(), (4, 5), &mut out)
        .unwrap();

    assert_eq!(apc_spans(&out).len(), 1, "small payload is a single APC");
    let (control, payload) = decoded_image(&out);
    let values = control_values(&control);
    assert_eq!(values.get("a"), Some(&"T"));
    assert_eq!(values.get("f"), Some(&"24"), "board layer is packed RGB");
    assert_eq!(values.get("o"), Some(&"z"));
    assert_eq!(values.get("i"), Some(&"7"));
    assert_eq!(values.get("p"), Some(&"3"), "pinned placement id");
    assert_eq!(values.get("c"), Some(&"20"), "well cell box");
    assert_eq!(values.get("r"), Some(&"19"));
    assert_eq!(values.get("C"), Some(&"1"), "never moves the cursor");
    assert_eq!(values.get("q"), Some(&"2"), "no replies on the tty");
    assert_eq!(values.get("z"), Some(&"1"), "layer stacking");
    assert_eq!(values.get("m"), Some(&"0"));
    assert_eq!(payload, wiring.data(), "pixels survive base64 + zlib");
    assert!(out.starts_with(b"\x1b[6;5H"), "cursor moves first");
}

#[test]
fn rgba_layers_declare_the_32_bit_format() {
    let mut layer = PixelCanvas::new(32, 16, PixelFormat::Rgba);
    layer.fill_rect(0, 0, 8, 8, Rgb::new(1, 2, 3));
    let mut writer = KittyEscapeWriter::new(support());
    let mut out = Vec::new();

    writer
        .transmit_native(&layer, 1, 1, 1, (0, 0), &mut out)
        .unwrap();

    let (control, payload) = decoded_image(&out);
    assert_eq!(control_values(&control).get("f"), Some(&"32"));
    assert!(
        !control_values(&control).contains_key("c"),
        "a native-size sprite must not ask for cell scaling"
    );
    assert_eq!(payload, layer.data());
}

#[test]
fn large_payloads_chunk_on_quartets_and_repeat_quiet_mode() {
    let geom = WellGeometry::new((20, 19), (720, 1600));
    let (w, h) = geom.pixels();
    let mut canvas = PixelCanvas::new(w, h, PixelFormat::Rgb);
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
        assert!(payload.len() <= BASE64_CHUNK, "chunk {index} too long");
        assert_eq!(payload.len() % 4, 0, "chunk {index} splits a quartet");
        assert!(control.contains("q=2"), "chunk {index} must stay quiet");
        let last = index + 1 == spans.len();
        assert!(
            control.contains(if last { "m=0" } else { "m=1" }),
            "{control}"
        );
        if index > 0 {
            assert!(
                !control.contains("a=T"),
                "continuation repeats control data"
            );
        }
    }
}

#[test]
fn tmux_passthrough_wraps_every_command_with_doubled_escapes() {
    let mut layer = PixelCanvas::new(32, 16, PixelFormat::Rgba);
    layer.fill_rect(0, 0, 8, 8, Rgb::new(1, 2, 3));
    let mut writer = KittyEscapeWriter::new(KittySupport {
        tmux_passthrough: true,
    });
    let mut out = Vec::new();
    writer
        .transmit_native(&layer, 1, 1, 1, (0, 0), &mut out)
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
            assert!(i + 1 < out.len(), "unterminated envelope");
            if out[i] == ESC {
                if out[i + 1] == ESC {
                    i += 2;
                    continue;
                }
                assert_eq!(out[i + 1], b'\\', "envelope ends with ST");
                cursor = i + 2;
                envelopes += 1;
                break;
            }
            i += 1;
        }
    }
    assert_eq!(envelopes, 2, "one envelope per graphics command");
}

// ---------------------------------------------------------------- geometry

#[test]
fn the_grid_tiles_the_box_and_keeps_minos_square() {
    // Whatever the cell shape, the bands must span the whole layer; otherwise
    // the bottom row floats above the frame or cells drift apart.
    for (cells, px) in [
        ((20u16, 19u16), (160u32, 323u32)),
        ((20, 20), (160, 340)),
        ((20, 19), (170, 344)),
        ((10, 20), (100, 240)),
    ] {
        let geom = WellGeometry::new(cells, px);
        let (x0, y0, band_w, band_h) = geom.cell_rect(0, 0);
        let (_, _, _, _) = (x0, y0, band_w, band_h);
        let (lx, ly, lw, lh) = geom.cell_rect(9, 19);
        assert!(band_w >= 4 && band_h >= 4, "{cells:?} {px:?}");
        assert!(
            lx + lw <= px.0 && ly + lh <= px.1,
            "grid fits {cells:?} {px:?}"
        );
        assert!(
            px.1 - (ly + lh) < band_h,
            "no leftover row: {cells:?} {px:?}"
        );
        assert!(
            px.0 - (lx + lw) < band_w,
            "no leftover column: {cells:?} {px:?}"
        );
    }
    assert_eq!(geometry().cells(), (20, 19));
    assert_eq!(geometry().pixels(), (160, 323));
}

// ------------------------------------------------------------- board layer

#[test]
fn settled_cells_touch_each_other_and_never_expose_the_backdrop() {
    let snap = board_with(&[(0, 17, 2), (1, 17, 2), (0, 18, 2), (1, 18, 2)]);
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    let canvas = well.board();
    // Every pixel of the 2x2 block is either body colour or its own rim.
    let (bx, by, _, bh) = geom.cell_rect(0, 17);
    let (ex, _, ew, _) = geom.cell_rect(1, 18);
    for y in by..by + 2 * bh {
        for x in bx..ex + ew {
            let pixel = canvas.pixel(x, y).expect("inside the layer");
            assert_ne!(pixel, WELL_BG, "backdrop pixel at ({x},{y})");
        }
    }
    let (x, y) = mino_center(geom, 9, 19);
    let empty = canvas.pixel(x, y).expect("empty cell");
    assert!(
        empty.r < 60 && empty.g < 60 && empty.b < 60,
        "empty cells keep the dark backdrop, got {empty:?}"
    );
    // A bottom-right mino exists and uses the piece colour.
    let snap = board_with(&[(9, 19, 3)]);
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();
    let (x, y) = mino_center(geom, 9, 19);
    assert_eq!(well.board().pixel(x, y), Some(FbRgb::new(158, 122, 235)));
}

#[test]
fn a_locked_tetromino_looks_exactly_like_the_falling_one() {
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
    well.sync(&locked, geom, (0, 0), &mut out).unwrap();
    let board = well.board();

    for &(bx, by) in &[(5u16, 8u16), (6, 8), (5, 9), (6, 9)] {
        let (mx, my, band_w, band_h) = geom.cell_rect(bx, by);
        for y in my..my + band_h {
            for x in mx..mx + band_w {
                if motion.alpha(x, y) != Some(255) {
                    continue;
                }
                assert_eq!(
                    motion.pixel(x, y),
                    board.pixel(x, y),
                    "locked pixel ({x},{y}) differs from the falling piece"
                );
            }
        }
    }
}

#[test]
fn unchanged_content_does_not_retransmit() {
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
fn the_motion_layer_is_transparent_with_a_shadowed_piece_and_hollow_ghost() {
    let snap = GameSnapshot {
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
    let (x, y) = mino_center(geom, 1, 1);
    assert_eq!(
        motion.alpha(x, y),
        Some(0),
        "empty area shows the board through"
    );
    // The O shape is offset by (+1, 0) from its origin cell.
    let (x, y) = mino_center(geom, 5, 2);
    assert_eq!(motion.pixel(x, y), Some(Rgb::new(245, 224, 94)));
    // The shadow sticks out below the block and stays translucent.
    let (mx, my, band_w, band_h) = geom.cell_rect(5, 3);
    let shadow = motion.alpha(mx + band_w / 2, my + band_h + 1);
    assert!(
        matches!(shadow, Some(alpha) if alpha > 0 && alpha < 255),
        "drop shadow below the piece: {shadow:?}"
    );
    // The ghost is a translucent silhouette with a denser rim.
    let (gx, gy, gw, gh) = geom.cell_rect(5, 10);
    let fill = motion.alpha(gx + gw / 2, gy + gh / 2);
    let rim = motion.alpha(gx, gy);
    assert!(matches!(fill, Some(alpha) if alpha > 0 && alpha < 128));
    assert!(matches!(rim, Some(alpha) if alpha > 0), "ghost rim");
}

#[test]
fn only_the_changed_layer_is_retransmitted() {
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let snap = started_snapshot();
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    // A locked cell moves only the board layer.
    let mut locked = snap;
    locked.board[19][0] = 1;
    let mut moved = locked;
    out.clear();
    well.sync(&locked, geom, (0, 0), &mut out).unwrap();
    let spans = apc_spans(&out);
    assert_eq!(spans.len(), 1, "one layer per change");
    let (control, _) = control_and_payload(spans[0]);
    assert_eq!(control_values(&control).get("z"), Some(&"0"), "board layer");

    // A falling row moves only the motion layer (continue from the locked board,
    // otherwise the board change would legitimately resend it too).
    moved.active = moved.active.map(|active| ActiveSnapshot {
        y: active.y + 1,
        ..active
    });
    out.clear();
    well.sync(&moved, geom, (0, 0), &mut out).unwrap();
    let (control, _) = control_and_payload(apc_spans(&out)[0]);
    let values = control_values(&control);
    assert_eq!(values.get("z"), Some(&"1"), "motion layer");
    assert_eq!(values.get("f"), Some(&"32"));
}

#[test]
fn a_clear_flashes_the_row_and_pausing_dims_the_well() {
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut full_row = [(0usize, 19usize, 1u8); BOARD_WIDTH as usize];
    for (x, cell) in full_row.iter_mut().enumerate() {
        *cell = (x, 19, (x % 7 + 1) as u8);
    }
    let mut snap = board_with(&full_row);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    snap.timers.line_clear_ms = 120;
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();
    let (x, y) = mino_center(geom, 0, 19);
    assert!(
        matches!(well.motion().alpha(x, y), Some(alpha) if alpha > 0),
        "the cleared row must flash"
    );

    snap.timers.line_clear_ms = 0;
    snap.paused = true;
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();
    let (x, y) = mino_center(geom, 5, 5);
    assert!(
        matches!(well.motion().alpha(x, y), Some(alpha) if alpha > 0),
        "pausing dims the whole well"
    );
}

// ----------------------------------------------------------------- hashes

#[test]
fn each_layer_hash_tracks_only_what_it_draws() {
    let mut snap = GameSnapshot::default();
    let board = board_hash(&snap);
    let motion = motion_hash(&snap, geometry());

    snap.score = 999;
    snap.level = 9;
    snap.next_queue[0] = PieceKind::S;
    assert_eq!(board_hash(&snap), board, "HUD fields are not board pixels");
    assert_eq!(motion_hash(&snap, geometry()), motion, "nor motion pixels");

    snap.board[0][0] = 1;
    assert_ne!(board_hash(&snap), board);
    assert_eq!(
        motion_hash(&snap, geometry()),
        motion,
        "board is not motion"
    );

    let falling = GameSnapshot {
        active: Some(ActiveSnapshot {
            kind: PieceKind::T,
            rotation: Rotation::North,
            x: 3,
            y: 0,
        }),
        ..GameSnapshot::default()
    };
    assert_ne!(motion_hash(&falling, geometry()), motion);
    assert_eq!(board_hash(&falling), board_hash(&GameSnapshot::default()));
}

// ---------------------------------------------------------------- lifecycle

#[test]
fn geometry_changes_resize_and_a_cleared_screen_forces_a_resend() {
    let snap = started_snapshot();
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    let resized = WellGeometry::new((10, 20), (100, 240));
    out.clear();
    well.sync(&snap, resized, (0, 0), &mut out).unwrap();
    assert_eq!(apc_spans(&out).len(), 2, "both layers resend");
    assert_eq!(well.board().width(), resized.pixels().0);

    out.clear();
    well.invalidate();
    well.sync(&snap, resized, (0, 0), &mut out).unwrap();
    assert_eq!(apc_spans(&out).len(), 2, "CSI 2J clears images");
}

#[test]
fn shutdown_removes_every_placement_once() {
    let snap = board_with(&[(0, 0, 1)]);
    let geom = geometry();
    let mut well = WellRenderer::new(support(), geom);
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    out.clear();
    well.invalidate(); // markers cleared, images still on screen
    well.shutdown(&mut out);
    let spans = apc_spans(&out);
    assert_eq!(spans.len(), 1);
    assert_eq!(
        control_and_payload(spans[0]).0,
        "a=d,d=A,q=2",
        "delete everything"
    );

    out.clear();
    well.shutdown(&mut out);
    assert!(out.is_empty(), "shutdown is idempotent");
}

// ----------------------------------------------------------------- previews

#[test]
fn preview_minos_are_half_a_board_mino_and_exact_squares() {
    let snap = GameSnapshot {
        hold: Some(PieceKind::O),
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
    previews.sync(&snap, (0, 0), (0, 0), &mut out).unwrap();

    let side = previews.mino();
    assert_eq!(side, 8, "half of a 16px board mino");
    assert_eq!(previews.next().width(), PREVIEW_COLS * side);
    assert_eq!(
        previews.next().height(),
        5 * (2 * side + 2 * side) - 2 * side,
        "five slots with a blank half slot between pieces"
    );
    assert_eq!(spans_of(&out), 2, "HOLD and NEXT are separate images");

    // The O preview is exactly two mino squares across and down.
    let canvas = previews.hold();
    let opaque = |x: u32, y: u32| canvas.alpha(x, y) == Some(255);
    let xs: Vec<u32> = (0..canvas.width())
        .filter(|x| (0..canvas.height()).any(|y| opaque(*x, y)))
        .collect();
    let ys: Vec<u32> = (0..canvas.height())
        .filter(|y| (0..canvas.width()).any(|x| opaque(x, *y)))
        .collect();
    assert_eq!(xs.len() as u32, 2 * side);
    assert_eq!(ys.len() as u32, 2 * side);
}

#[test]
fn previews_only_restyle_when_the_queue_or_anchor_moves() {
    let snap = GameSnapshot {
        next_queue: [PieceKind::O; 5],
        ..GameSnapshot::default()
    };
    let mut previews = PreviewRenderer::new(support(), 16);
    let mut out = Vec::new();
    previews.sync(&snap, (0, 0), (0, 0), &mut out).unwrap();

    let mut scored = snap;
    scored.score = 1234;
    out.clear();
    assert!(!previews.sync(&scored, (0, 0), (0, 0), &mut out).unwrap());
    assert!(out.is_empty(), "score changes are text-only");

    let mut queued = snap;
    queued.next_queue[4] = PieceKind::T;
    out.clear();
    assert!(previews.sync(&queued, (0, 0), (0, 0), &mut out).unwrap());
    assert_eq!(spans_of(&out), 1, "only the queue strip resends");

    out.clear();
    assert!(previews.sync(&queued, (0, 0), (0, 4), &mut out).unwrap());
    assert_eq!(spans_of(&out), 1, "a new anchor re-places the strip");

    out.clear();
    previews.shutdown(&mut out);
    assert_eq!(spans_of(&out), 1);
    out.clear();
    previews.shutdown(&mut out);
    assert!(out.is_empty(), "shutdown is idempotent");
}

fn spans_of(out: &[u8]) -> usize {
    apc_spans(out).len()
}

#[test]
fn the_board_is_the_same_size_as_the_play_area() {
    // Guards the constants the smoke test relies on.
    assert_eq!(BOARD_WIDTH, 10);
    assert_eq!(BOARD_HEIGHT, 20);
}
