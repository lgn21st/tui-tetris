# Kitty Graphics Backend

Ghostty 1.4 implements 100% of the Kitty graphics protocol
(<https://sw.kovidgoyal.net/kitty/graphics-protocol/>), so the playfield and the
HUD previews are drawn as pixels. There is no cell-based board renderer any more,
and the interactive modes refuse to start without a working backend: an empty
well is worse than a clear error. Text (labels, score, banners) stays text,
because the terminal font renders it better than a bitmap font would.

## Layers

```text
board layer    RGB   z=0   backdrop + settled minos
motion layer   RGBA  z=1   piece, drop shadow, ghost, lock tint, flash, dim
hold preview   RGBA  z=1   HOLD piece, native size
next preview   RGBA  z=1   five NEXT pieces, native size
```

Splitting by change frequency is what makes per-frame animation affordable: the
opaque board is re-sent only on a lock or a clear, the mostly transparent motion
layer on every piece movement. The falling piece is drawn *between* rows: the
renderer learns the gravity period from `timers.drop_ms`, so a whole period is
spent crossing the band instead of jumping a row on the tick (about 16
retransmissions a second, quantised to whole pixels). Each layer keeps a content hash and a stable image
and placement id, so a retransmission replaces the previous placement instead of
stacking one.

## Geometry

- **The frame hugs the pixel grid.** Twenty square minos of `side` pixels need
  `ceil(800 / cell_px_h)` text rows - 19 for an 8x17 cell, not 20 - and the image
  is sent at the terminal's own device resolution (160x323 px into a 20x19 cell
  box, scale factor 1). Minos are exactly 16x16 with no resampling.
- **Bands tile the box.** Each board cell fills its band edge to edge, so no
  backdrop pixel ever shows between cells.
- **One painter for both states.** The rim is drawn only where a region has no
  neighbour of the same colour, so a tetromino reads as one block and a locked
  piece is pixel identical to the same piece while falling.
- **Previews are half a board mino**, derived from the device pixel size so they
  scale with the display (8 px on an 8x17 cell, 16 px on a Retina cell), with a
  blank half slot between pieces.

`GameView::panel_layout` gives the text pass and the sprite layers the same
anchors, bottom-anchored so the combo/B2B rows never move a sprite.

## Wire decisions

| Decision | Why |
|----------|-----|
| `a=T` with pinned `p` id | A retransmission replaces one placement instead of accumulating copies. |
| `f=24` RGB board / `f=32` RGBA motion | The board is opaque; the motion layer must composite over it. |
| `o=z` zlib | 163 KB of flat colour becomes about 3 KB. |
| 4096-byte base64 chunks | Protocol limit, and a multiple of four so no quartet is split. |
| `q=2` on **every** chunk | A continuation without it makes the terminal answer `Gi=...;OK` onto the tty, which tmux prints into the prompt. |
| `C=1` | Placement must not move the cursor; the diff renderer positions it. |
| `z=0` / `z=1` | Board below, motion and previews above. |
| `a=d,d=A,q=2` on exit | Frees the terminal's image storage. |

A full-screen text redraw emits `CSI 2J`, which also clears images, so
`Graphics::invalidate()` forces the next sync to resend.

## tmux

tmux does not render graphics itself; it forwards them only through
`set -g allow-passthrough on`, wrapping each command in
`ESC Ptmux; <payload with every ESC doubled> ESC \`. One envelope per command,
because a single envelope for a chunked image overflows what tmux buffers and is
dropped silently. The game checks `tmux show -gv allow-passthrough` at startup
and refuses to start when it is off.

`TUI_TETRIS_TMUX_PASSTHROUGH=on|off` and `TUI_TETRIS_TMUX_CLIENT=xterm-ghostty`
override the probes (useful for harnesses). Detection covers kitty, Ghostty,
WezTerm and Rio; `TUI_TETRIS_GRAPHICS=kitty` forces it, `=off` disables it, and
`TUI_TETRIS_CELL_PX_W`/`_H` override the measured cell size.

## Cost

| Path | Cost | Frequency |
|------|------|-----------|
| board layer | ~190 us, ~3 KB | lock or clear |
| motion layer | ~44 us, ~0.4 KB | each piece movement |
| nothing changed | 0.22 us | the common case |
| HOLD preview | ~60 B | held piece changes |
| NEXT preview | ~1 KB | once per lock |

`cargo bench --bench game_logic -- kitty_` covers the layers and the no-op path;
`tests/term_no_alloc_gate_test.rs` asserts `sync` does not allocate after warm-up.

## Verifying without a terminal

```bash
cargo run --release --example well_layers -- /tmp   # board.rgb + motion.rgba
python3 scripts/kitty_smoke_test.py                  # wire + shape checks on a pty
python3 scripts/kitty_smoke_test.py --tmux           # passthrough envelopes
python3 scripts/kitty_smoke_test.py --graphics off   # must refuse to start
```

The smoke test checks the chunking, quiet mode, envelopes, pixel formats, that
the motion layer is sparse and the board opaque, and (with
`--expect-square-minos`) that every fully opaque block is square in grid units.
Composite the dumped layers to inspect rendering by eye.

## Limits

- Each layer is a full-well image, so one changed pixel re-sends that layer. A
  per-mino sprite atlas would cut the motion layer to ~90 bytes per move and
  allow sub-cell motion, but needs pixel-offset arithmetic that can drift against
  the board.
- No animation frames (`a=f`): fades and the sub-cell fall are driven by the
  existing 16 ms tick and the snapshot's `timers.drop_ms`.
- Images do not scroll with text; the well lives on the alternate screen, so
  `U=1` Unicode placeholders (the tmux scroll-safe mechanism) are not needed.
- The side panel needs 12 columns beside the frame; below that it is skipped with
  a one-line stderr hint.
