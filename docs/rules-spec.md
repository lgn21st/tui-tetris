# tui-tetris Rules Implementation Profile

Portable rules: [`protocol/rules/SPEC.md`](../protocol/rules/SPEC.md).
Replay id: `guideline-ds-1.0.0`.

This file is local policy only. Other stacks may differ. Shared geometry,
SRS (wiki Y-up), scoring, lock delay, T-Spin, B2B, and combo live in the
SPEC. This engine stores kicks with **+x right, +y down** (negate wiki `dy`).

## Board

- Visible 10×20, no hidden rows
- Spawn `(3, 0)` on the visible field
- Next queue length: 5
- Engine level: `floor(lines / 10)`, starting at 0

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

`DROP_INTERVALS`: 1000, 800, 650, 500, 400, 320, 250, 200, 160; level 9+ is 120 ms.

- Fixed step `TICK_MS`; retain backlog, at most 8 catch-up steps per outer loop
- Per step: remote commands, then local DAS/ARR, then one tick
- `LINE_CLEAR_PAUSE_MS` pauses gravity only; the next piece is already spawned and can move
- When `line_clear_ms` hits 0, gravity/lock resume in the same `tick()`
- Gravity accumulator may drop multiple rows in one tick
- Lock timer runs only while grounded; grounded moves/rotates reset it up to `LOCK_RESET_LIMIT`

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
