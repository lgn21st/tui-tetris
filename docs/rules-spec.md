# tui-tetris Rules Implementation Profile

Portable rules: [`protocol/rules/SPEC.md`](../protocol/rules/SPEC.md).
Replay id: `guideline-ds-1.1.0+tui-tetris-timing-1.1.0`. The portable ruleset
version and this profile's timing id are recorded together; the composite is
what decides whether a replay can be verified.

This file is local policy only. Other stacks may differ. Shared geometry,
SRS (wiki Y-up), scoring, lock delay, T-Spin, B2B, and combo live in the
SPEC. This engine stores kicks with **+x right, +y down** (negate wiki `dy`).

## Board

- Visible 10×20, no hidden rows
- Spawn `(3, 0)` on the visible field
- Next queue length: 5
- Engine level: `floor(lines / 10)`, starting at 0
- HUD shows guideline level (`engine_level + 1`)
- Minos are pixels, not terminal cells: the well is rendered through the Kitty
  graphics protocol and the game refuses to start without it. See
  `docs/kitty-graphics.md`. Rendering only, no rule effect.
- Hold/next HUD minos are pixel sprites at half a board mino each (8x8 device px
  on an 8x17 cell, 16x16 on a Retina cell, i.e. they scale with the display),
  placed by the graphics backend rather than half-block glyphs, so their shape
  does not depend on the terminal font.

## Timing

| Constant | Value |
|----------|-------|
| TICK_MS | 16 |
| BASE_DROP_MS | 1000 |
| SOFT_DROP_MULTIPLIER | 10 |
| SOFT_DROP_GRACE_MS | 150 |
| LOCK_DELAY_MS | 500 |
| LOCK_RESET_LIMIT | 15 |
| LINE_CLEAR_PAUSE_MS | 180 |
| LANDING_FLASH_MS | 120 |

`DROP_INTERVALS` follows the Guideline gravity formula
`(0.8 - (level - 1) * 0.007) ^ (level - 1)` seconds per row, rounded to whole
milliseconds for engine levels 0..14:

1000, 793, 618, 473, 355, 262, 190, 135, 94, 64, 43, 28, 18, 11, 7

Level 15+ uses `DROP_INTERVAL_FLOOR_MS` (7 ms ≈ 2.4G). Official games keep
normal modes near speed level 15 and reserve 20G for level 19+ or a separate
high-level mode; 20G is only playable there because those modes add Infinity or
a decreasing lock delay. The portable ruleset forbids Infinity (§6) and this
profile keeps a fixed lock delay, so the normal-mode curve must not run to 20G.

Soft drop divides the interval by `SOFT_DROP_MULTIPLIER` and MUST clamp to at
least 1 ms; a zero interval would make the gravity accumulator loop forever.

- Fixed step `TICK_MS`; retain backlog, at most 8 catch-up steps per outer loop
- Per step: remote commands, then local DAS/ARR, then one tick
- `LINE_CLEAR_PAUSE_MS` pauses gravity only; the next piece is already spawned and can move
- When `line_clear_ms` hits 0, gravity/lock resume in the same `tick()`
- Gravity accumulator may drop multiple rows in one tick
- Lock timer runs only while grounded; grounded moves/rotates reset it up to `LOCK_RESET_LIMIT`

## Timing profile id

The portable ruleset excludes gravity and tick timing (`protocol/rules/SPEC.md`
§10), so this profile carries its own id, `tui-tetris-timing-1.1.0`. Replay
metadata and `welcome.ruleset_id` record the composite
`guideline-ds-1.1.0+tui-tetris-timing-1.1.0`. Changing any constant in this
section changes deterministic trajectories and MUST bump the timing profile id.

## DAS/ARR

- DAS 150 ms, ARR 50 ms
- Soft drop: DAS 0 ms, ARR 50 ms
- No key-release: `TUI_TETRIS_KEY_RELEASE_TIMEOUT_MS` (default 150)
- Repeat-but-no-release clamp: `TUI_TETRIS_REPEAT_RELEASE_TIMEOUT_MIN_MS` / `_MAX_MS` (80 / 300)

## Adapter events

- `events[].line_clear_score` is base clear (with B2B), excluding combo and drop
- 0-line T-Spin still scores; `events[]` omit `tspin` when `lines_cleared = 0`

## RNG

7-bag (portable) shuffled with LCG `a=1664525`, `c=1013904223`, `m=2^32`.

## Pause / game over

- Paused: only `Pause` and `Restart`
- Game over: only `Restart`
