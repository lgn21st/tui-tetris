# Kitty Graphics Backend

Ghostty 1.4.0 (October 2026) is the first non-Kitty terminal to implement 100%
of the published Kitty graphics protocol, including animation and relative
placements, with a SIMD payload parser. The playfield therefore renders as
pixels, not character cells.

- Upstream: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>
- Release notes: <https://ghostty.org/docs/install/release-notes/1-4-0>

## Rendering architecture

There is no cell-based board renderer any more. The interactive modes require a
working pixel backend and refuse to start without one, because an empty well is
worse than a clear error.

```text
GameSnapshot
   |-- board layer    RGB   z=0   well backdrop + settled minos
   |-- motion layer   RGBA  z=1   piece, drop shadow, ghost outline,
   |                              lock tint, line-clear flash, pause dim
   |-- hold preview   RGBA  z=1   HOLD piece, native size
   |-- next preview   RGBA  z=1   five NEXT pieces, native size
   `-- text cells                 labels, score/level/lines, borders, banners
```

`HOLD` and `NEXT` are pixels too. As half-block glyphs their mino height came
from the font (measured 7.5x8.75 device px on an 8x17 cell, i.e. 17% too tall);
`PreviewRenderer` draws them as pixel squares at native resolution. A preview
mino is **half a board mino**, derived from the terminal's device pixel size -
the same proportion the half-block previews had - so the previews scale with the
display instead of shrinking on a HiDPI screen:

| terminal cell | board mino | preview mino | HOLD image | NEXT strip |
|---------------|-----------|--------------|------------|------------|
| 8x17 | 16 px | 8 px | 32x16 | 32x144 |
| 16x34 (Retina) | 32 px | 16 px | 64x32 | 64x288 |

Both reserve the same 4x1 and 4x9 cell boxes, so the layout is identical at any
scale. A blank half slot (twice the preview mino) separates the pieces, matching
the blank text row the cell renderer used between NEXT entries.

The side panel keeps the cell renderer's composition: the stats stack in the
right column with blank separators (`SCORE` label, then its value on its own
row, `LEVEL`, `LINES`, optional `COMBO`/`B2B`), and `HOLD`/`NEXT` sit underneath
with their pixel previews.

Splitting the picture by *change frequency* is what makes per-frame animation
affordable. The opaque board layer is re-sent only when the settled board changes
(a lock or a clear); the mostly transparent motion layer deflates to a few
hundred bytes, so it can be re-sent on every piece movement.

Text stays text on purpose: the terminal's own font renders the HUD, labels and
banners better than any bitmap font we could ship, and it costs nothing to
update. Only the playfield became pixel-native.

The HUD anchors come from `GameView::panel_layout`, which both the text pass and
the preview layers call, so the labels and the sprites cannot drift apart. The
reserved cell box is computed from the preview pixel size
(`PreviewRenderer::hold_cell_box` / `next_cell_box`), which is 4x1 and 4x9 cells
at any display scale.

The previews are anchored to the bottom of the frame, so the combo/B2B rows above
them never move the sprites (a moving anchor would force a re-placement).

`tui-tetris diagnostic` prints the resolved state on a `graphics=` line, e.g.
`graphics=kitty,direct,cell=8x17px,well=160x340px,mino=16px`, which is the
quickest way to confirm what a given terminal/session selected.

## What the backend does

1. `WellGeometry` turns the terminal's cell grid into a device-pixel box: the
   well is `10 × cell_w` by `20 × cell_h` cells, and its pixel size comes from
   `window pixels / columns`, not from a floored integer cell size. One mino is
   the largest square that fits (`mino_side`), centred in the box, so minos are
   exactly square with no stretch.
2. `paint_board` fills the board layer with the well background and one
   bevel-and-grout block per settled cell.
3. `paint_backdrop` lays down the well interior: a barely-there vertical
   gradient (`WELL_BG` to `WELL_BG_BOTTOM`) plus a one pixel inset edge line, so
   the well has depth instead of being flat dead space. One constant colour per
   row keeps it free to compress.
4. `paint_motion` clears the motion layer to transparent and draws, in order:
   - a soft offset drop shadow (`SHADOW_ALPHA` 118 at `SHADOW_OFFSET` 1x2) so the
     piece reads as a block above the stack without scarring it black;
   - the ghost as a **translucent silhouette** (`GHOST_FILL_ALPHA` 52 plus a
     `GHOST_EDGE_ALPHA` 84 rim), which is readable over the empty well and calm
     where it crosses a bright locked mino;
   - the active piece through the same region painter as the stack, only with a
     drop shadow underneath and a whitening tint while the lock timer runs;
   - the line-clear flash bands, and the pause/game-over dim.

   Aesthetic constants live at the top of `well.rs`. Note that with an 8x17
   terminal cell a square mino cannot leave equal gaps on both axes: the bands
   are 16x17, so gaps are 2 px horizontally and 3 px vertically. Shrinking the
   square does not fix it (it widens the horizontal gap first), so the design
   keeps the square as large as possible.
4. `board_hash` / `motion_hash` decide which layer, if either, is retransmitted.
   Both reuse a stable image id and placement id, so a retransmission replaces
   the previous placement instead of stacking another one.
5. `KittyEscapeWriter` zlib-deflates (`o=z`), base64-encodes, and emits chunked
   APCs placed at the well origin with `c=`/`r=` set to the well's cell box.

Nothing in `tetris-core`, `tetris-session`, or the adapter changes: the backend
consumes the same immutable snapshot, and replays stay byte-identical.

## Mino shape

The character renderer could only use whole cells per mino, so its minos were
whatever rectangle `cell_w x cell_h` cells happened to be: 6.2 % off for an 8x17
cell, 7.1 % for 7x15, 18.8 % for 8x19.

The pixel grid removes the quantisation entirely. The well's device-pixel box is
split into 10x20 **bands** (`band_pixels`), and each band holds the largest
square that fits it (`mino_side`) centred in place (`mino_origin`):

- the frame **hugs the pixel grid**. Twenty square minos of `side` pixels need
  `ceil(20 * side / cell_px_h)` text rows; for an 8x17 cell that is 19 rows, not
  20. `GameView::with_well_cells` reserves that box, so the image is sent at the
  terminal's own device resolution (160x323 px into a 20x19 cell box, scale
  factor exactly 1) and the bands come out 16x16 - square minos with no
  resampling and only a 1-2 px margin at the frame;
- every cell **fills its band completely**, edge to edge. No grout ring, no
  background between cells: the two earlier attempts each left 1-3 px of well
  background around every cell, which made a tetromino look like four loose
  tiles. Verified by a test that scans the whole board for a backdrop pixel
  trapped between two coloured pixels (must be zero);
- there is **one painter for both states**. A falling piece and the same piece
  after it locks run through `paint_regions` with the same colours and the same
  bevel weights, so settling changes nothing but the disappearance of the drop
  shadow. `a_locked_tetromino_looks_exactly_like_the_falling_one` asserts the
  pixels are identical;
- depth comes from a **region rim**, not from per-cell borders: the light/dark
  bevel is drawn only where a region has no neighbour of the same colour, so a
  tetromino is one solid block and adjacent cells of the same colour merge.

The image is drawn into the terminal's cell box with `c=`/`r=`; because its size
is derived from `window pixels / columns`, the scale factor is ~1 and no residual
stretch is left.

`cargo run --release --example well_layers -- /tmp` writes `board.rgb` and
`motion.rgba` for a representative mid-game frame, so the rendering can be
inspected (and diffed) without a terminal.

`scripts/kitty_smoke_test.py --expect-square-minos` decodes the transmitted
image, takes the fully opaque blocks (the shadow, ghost and flashes are
translucent), and fails unless every block is as wide as it is tall; a stretched
grid reports e.g. `mino block is 14x16 px, not square`.

## Protocol decisions

These are the details that decide whether the feature works on a live terminal.

| Decision | Why |
|----------|-----|
| `a=T` transmit-and-place with a pinned `p` id | A retransmission replaces one placement instead of accumulating ghost copies. |
| Two layers at `z=0` and `z=1` | Change frequency differs by ~20x; the board must not be re-deflated for every falling row. |
| `f=24` packed RGB for the board | A quarter fewer bytes than RGBA; the board is opaque. |
| `f=32` RGBA for the motion layer | The piece must composite over the board; transparent pixels make the layer compress to a few hundred bytes. |
| `o=z` zlib deflate | 160×340 RGB (163 KB) compresses to ≈3 KB of pixel art. |
| `i=` fixed image id `0x74545253` | Distinctive so the game never deletes another program's images. |
| Chunks of 4096 base64 bytes | Protocol limit, and a multiple of four so a quartet is never split. |
| `q=2` on **every** chunk | A continuation without it makes the terminal answer `Gi=...;OK` onto the tty, which tmux then prints into the prompt. |
| `C=1` | Placement must not move the text cursor; the diff renderer positions the cursor itself. |
| Delete before transmitting on geometry change | The old image has a different aspect; leaving it would double-draw during the transition. |
| `a=d,d=A,q=2` on shutdown | Frees the terminal's image storage when leaving the alternate screen. |

A full-screen redraw emits `CSI 2J`, which by specification also clears images.
`Graphics::invalidate()` therefore forces the next `sync` to retransmit; it is
called wherever `TerminalRenderer::invalidate()` is called (resize, first frame).

## tmux

tmux 3.8 does not render Kitty graphics itself; it forwards them only through
the passthrough envelope:

```text
ESC P tmux; <payload with every ESC doubled> ESC \
```

Each graphics command needs its own envelope (a single envelope for a chunked
image overflows what tmux buffers and is dropped silently). Outside tmux a bare
APC is fine. `KittySupport::tmux_passthrough` selects the framing.

Requirements inside tmux:

```tmux
set -g allow-passthrough on
```

Running directly in Ghostty (no multiplexer) needs no configuration at all.

Security tradeoff: passthrough lets any program in a pane emit escapes straight
to the outer terminal, so tmux defaults it to `off`. The game checks
`tmux show -gv allow-passthrough` at startup and disables graphics (with a hint
on stderr) instead of burning CPU on escapes tmux will swallow.

Cursor-positioned images are owned by the outer terminal, so a pane split or a
scroll can leave the image where it was. For a single-pane alt-screen game this
is bounded: resize sends `SIGWINCH`, which triggers a full redraw, a geometry
change, and therefore a fresh transmission.

`U=1` Unicode placeholders (`U+10EEEE` plus row/column diacritics) are the
fully-robust tmux mechanism, and are the natural next step if the game ever
paints into a scrolling region.

## Enabling and tuning

| Variable | Effect |
|----------|--------|
| `TUI_TETRIS_GRAPHICS` | `kitty`/`on`/`force`/`1` forces the backend on; `off` makes the game refuse to start (the playfield has no cell fallback). |
| `TUI_TETRIS_TMUX_PASSTHROUGH` | Overrides the `tmux show -gv allow-passthrough` query (`on`/`off`). |
| `TUI_TETRIS_TMUX_CLIENT` | Overrides `tmux display-message -p '#{client_termname}'` for outer-terminal detection. |
| `TUI_TETRIS_CELL_PX_W` / `_H` | Pixel size of one terminal cell, when the tty does not report it. |
| `TUI_TETRIS_CELL_W` / `_H` | Terminal cells per mino, as before; the image follows the same geometry. |

Recognition is strict and covers kitty (`KITTY_WINDOW_ID`, `TERM=*kitty*`),
Ghostty (`TERM_PROGRAM=ghostty` or the `GHOSTTY_*` paths, which survive into
tmux), WezTerm, and Rio. Everything else reports unsupported. Inside tmux the
outer terminal name is read from
`tmux display-message -p '#{client_termname}'`.

The cell grid is required: if `TIOCGWINSZ` does not report the window's pixel
size, mino size would be a guess, so the game refuses to start instead.

## Measured cost

Release build, 160×340 device-pixel well (2×1 cells at 8×17 px):

| Path | Cost | Frequency |
|------|------|-----------|
| board layer retransmit | ≈190 µs, ≈3 KB | on lock or clear (≈2/s) |
| motion layer retransmit | ≈44 µs, ≈0.4 KB | on every piece movement |
| nothing changed | 0.22 µs | the common case |
| HOLD preview | ~40 B | when the held piece changes |
| NEXT preview | ~640 B | once per lock |

A busy second (10 piece moves, 2 locks) is therefore ≈0.8 ms of CPU and ≈10 KB
of terminal input. The previous single-image design spent 115 µs and 3 KB on
*every* change because the whole opaque well had to be re-deflated; layering cut
the per-frame cost by roughly 4x and the per-frame payload by 7x.

`cargo bench --bench game_logic -- kitty_` covers each layer plus the no-op path,
and `tests/term_no_alloc_gate_test.rs` asserts that `sync` does not allocate
after warm-up.

## Verification without a real terminal

`scripts/kitty_smoke_test.py` runs the game on a pseudo-terminal with a known
pixel size, captures the byte stream, and checks that:

- the alternate screen is entered and left,
- every graphics payload is a well-formed, quiet, chunked APC with the expected
  pixel format, `c`/`r` cell box and `z` index,
- the board layer decodes to opaque RGB containing the well background,
- the motion layer decodes to RGBA that is translucent and sparse,
- with `--expect-square-minos`, every fully opaque block on the motion layer is
  as wide as it is tall (a stretched grid is reported as `14x16 px, not square`),
- inside tmux every command is wrapped in a `DCS tmux;` envelope and no bare APC
  survives.

```bash
cargo build
python3 scripts/kitty_smoke_test.py
python3 scripts/kitty_smoke_test.py --tmux
python3 scripts/kitty_smoke_test.py --graphics off
```

## Limits and next steps

- Each layer is a full-well image, so a single pixel change re-sends that whole
  layer. A per-mino sprite atlas placed with `a=p` would cut the motion layer to
  ~90 bytes per move and would allow sub-cell smooth motion; the current design
  was chosen because it needs no pixel-offset arithmetic and therefore cannot
  drift against the board layer.
- No animation frames (`a=f`) are used yet; line-clear fades are driven by the
  existing 16 ms tick instead.
- Images do not scroll with text; the well lives on the alternate screen.
- The character renderer is gone, so a non-Kitty terminal cannot run the game.
