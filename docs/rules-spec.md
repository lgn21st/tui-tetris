# Tetris Rules Specification

Comprehensive rules and timing constants for tui-tetris.
This document is the source of truth for gameplay rules/timing constants.

## Board

- **Width**: 10 cells
- **Height**: 20 cells  
- **Spawn Position**: (x=3, y=0) - top-left of piece bounding box

## Tetrominoes

### Shapes (mino offsets from piece origin)

**I Piece:**
- North: [(0,1), (1,1), (2,1), (3,1)]
- East:  [(2,0), (2,1), (2,2), (2,3)]
- South: [(0,2), (1,2), (2,2), (3,2)]
- West:  [(1,0), (1,1), (1,2), (1,3)]

**O Piece:**
- All rotations: [(1,0), (2,0), (1,1), (2,1)]

**T Piece:**
- North: [(1,0), (0,1), (1,1), (2,1)]
- East:  [(1,0), (1,1), (2,1), (1,2)]
- South: [(0,1), (1,1), (2,1), (1,2)]
- West:  [(1,0), (0,1), (1,1), (1,2)]

**S Piece:**
- North: [(1,0), (2,0), (0,1), (1,1)]
- East:  [(1,0), (1,1), (2,1), (2,2)]
- South: [(1,1), (2,1), (0,2), (1,2)]
- West:  [(0,0), (0,1), (1,1), (1,2)]

**Z Piece:**
- North: [(0,0), (1,0), (1,1), (2,1)]
- East:  [(2,0), (1,1), (2,1), (1,2)]
- South: [(0,1), (1,1), (1,2), (2,2)]
- West:  [(1,0), (0,1), (1,1), (0,2)]

**J Piece:**
- North: [(0,0), (0,1), (1,1), (2,1)]
- East:  [(1,0), (2,0), (1,1), (1,2)]
- South: [(0,1), (1,1), (2,1), (2,2)]
- West:  [(1,0), (1,1), (0,2), (1,2)]

**L Piece:**
- North: [(2,0), (0,1), (1,1), (2,1)]
- East:  [(1,0), (1,1), (1,2), (2,2)]
- South: [(0,1), (1,1), (2,1), (0,2)]
- West:  [(0,0), (1,0), (1,1), (1,2)]

## Rotation System (SRS)

### Wall Kick Tables

**JLSTZ Pieces:**
```
0→1 (N→E):  [(0,0), (-1,0), (-1,1), (0,-2), (-1,-2)]
0→3 (N→W):  [(0,0), (1,0), (1,1), (0,-2), (1,-2)]
1→0 (E→N):  [(0,0), (1,0), (1,-1), (0,2), (1,2)]
1→2 (E→S):  [(0,0), (1,0), (1,-1), (0,2), (1,2)]
2→1 (S→E):  [(0,0), (-1,0), (-1,1), (0,-2), (-1,-2)]
2→3 (S→W):  [(0,0), (1,0), (1,1), (0,-2), (1,-2)]
3→2 (W→S):  [(0,0), (-1,0), (-1,-1), (0,2), (-1,2)]
3→0 (W→N):  [(0,0), (-1,0), (-1,-1), (0,2), (-1,2)]
```

**I Piece:** (Different kicks)
```
0→1 (N→E): [(0,0), (-2,0), (1,0), (-2,-1), (1,2)]
0→3 (N→W): [(0,0), (-1,0), (2,0), (-1,2), (2,-1)]
1→0 (E→N): [(0,0), (2,0), (-1,0), (2,1), (-1,-2)]
1→2 (E→S): [(0,0), (-1,0), (2,0), (-1,2), (2,-1)]
2→1 (S→E): [(0,0), (1,0), (-2,0), (1,-2), (-2,1)]
2→3 (S→W): [(0,0), (2,0), (-1,0), (2,1), (-1,-2)]
3→2 (W→S): [(0,0), (-2,0), (1,0), (-2,-1), (1,2)]
3→0 (W→N): [(0,0), (1,0), (-2,0), (1,-2), (-2,1)]
```

**O Piece:** No kicks - [(0,0)]

## Timing

| Constant | Value | Description |
|----------|-------|-------------|
| TICK_MS | 16 | Logic update interval (60 FPS) |
| BASE_DROP_MS | 1000 | Level 0 drop interval |
| SOFT_DROP_MULTIPLIER | 10 | Soft drop speed multiplier |
| SOFT_DROP_GRACE_MS | 150 | Soft drop state timeout |
| LOCK_DELAY_MS | 450 | Time before piece locks |
| LOCK_RESET_LIMIT | 15 | Max lock delay resets per piece |
| LINE_CLEAR_PAUSE_MS | 180 | Pause duration after clearing |
| LANDING_FLASH_MS | 120 | Landing flash duration |

### Drop Intervals by Level

| Level | Interval (ms) |
|-------|---------------|
| 0 | 1000 |
| 1 | 800 |
| 2 | 650 |
| 3 | 500 |
| 4 | 400 |
| 5 | 320 |
| 6 | 250 |
| 7 | 200 |
| 8 | 160 |
| 9+ | 120 |

### Fixed-Step Semantics

- The simulation runs at a fixed timestep of `TICK_MS` (16ms).
- `step_in_piece` increments once per fixed step while an active piece exists, including while `LINE_CLEAR_PAUSE_MS` is counting down.
- When `line_clear_ms` reaches `0` during a tick, gameplay resumes in the **same** `tick()` call (gravity/lock may advance immediately).
- Gravity uses an accumulator (while-loop): if `elapsed_ms` spans multiple drop intervals, multiple row drops may occur in one tick.
- Lock delay timing is grounded-only:
  - While the active piece can still move down, `lock_ms` and `lock_reset_count` stay at `0`.
  - When the active piece is grounded, `lock_ms` increases each step.
  - Successful moves/rotations that result in a grounded active piece reset `lock_ms` and consume up to `LOCK_RESET_LIMIT` resets per piece.
    - This means the gravity step that moves a piece into its first grounded position may consume the first lock reset.

### DAS/ARR

- **DAS (Delayed Auto Shift)**: 150ms
- **ARR (Auto Repeat Rate)**: 50ms
- Soft drop repeat: DAS=0ms, ARR=50ms.
- Note: terminals without key-release events use a timeout-based auto-release in the input handler.
  - Config: `TUI_TETRIS_KEY_RELEASE_TIMEOUT_MS` (default: 150ms)
  - If the terminal emits key repeat events (but no key release events), the input handler switches to a repeat-driven auto-release timeout derived from the observed repeat cadence, so movement stops quickly after repeats stop without breaking slower repeat rates.
    - Optional clamp: `TUI_TETRIS_REPEAT_RELEASE_TIMEOUT_MIN_MS` (default: 80ms) and `TUI_TETRIS_REPEAT_RELEASE_TIMEOUT_MAX_MS` (default: 300ms)
  - Tuning:
    - For “tap should move once” on terminals without key-release events, keep this below `DAS` (150ms).
    - For “hold should repeat” without terminal key-repeat events, set this above `DAS` (150ms) (or use a terminal that emits key-repeat events).

## Scoring

This project uses a classic base line-clear table (40/100/300/1200 * (level+1)) plus modern extensions (T-Spin tables, back-to-back, combo, and drop scoring).
This is closer to modern guideline-style scoring than legacy variants like NES.

### Classic Line Clear

| Lines | Base Score |
|-------|------------|
| 1 | 40 |
| 2 | 100 |
| 3 | 300 |
| 4 | 1200 |

Formula: `base_score * (level + 1)`

### T-Spin Scoring

| Type | Lines | Score |
|------|-------|-------|
| Full | 0 | 400 |
| Full | 1 | 800 |
| Full | 2 | 1200 |
| Full | 3 | 1600 |
| Mini | 0 | 100 |
| Mini | 1 | 200 |
| Mini | 2 | 400 |

Notes:
- When a T-Spin is detected (Full/Mini), the T-Spin table score is used for the clear (it does **not** add the classic line-clear score on top).
- Scoring uses the **pre-clear** level (i.e., level before adding the newly-cleared lines).
- T-Spin with **0 lines cleared** awards points (Full: 400, Mini: 100, multiplied by `level+1`), but it does not count as a line clear:
  - Combo/B2B chains reset.
  - Adapter `last_event` does not report a T-Spin for `lines_cleared=0`, and `line_clear_score` remains `0` (score still increases).

### Combo Bonus

`combo_bonus = 50 * combo_index`

Where:
- `combo_index` starts at `0` on the first clear in a chain (no bonus), then `1, 2, 3, ...` for consecutive clears.
- When a piece locks with `lines_cleared = 0`, the combo chain resets (`combo_index = -1` for diagnostics/adapter event reporting).
- Combo bonus is added **after** the base clear score (and after any B2B multiplier). It does not have a level multiplier.
  - In adapter observations, `last_event.line_clear_score` refers to the base clear score (including any B2B multiplier) and explicitly excludes combo and drop points.

### Back-to-Back

- **Qualifies**: T-spin full with lines OR Tetris (4 lines)
- **Bonus**: 1.5× base clear score (applies only to consecutive qualifying clears; multiplier is applied before combo bonus)

### Drop Scoring

- **Soft Drop**: +1 per cell
- **Hard Drop**: +2 per cell

Note: soft drop points are awarded for explicit soft-drop actions (moving down by one cell). Accelerated gravity during a soft-drop window does not add extra points.

## T-Spin Detection

A T-Spin is detected when:
1. Piece is T-shaped
2. Last action was a rotation
3. Piece is locked (cannot move down)
4. At least 3 of 4 corners around T-center are filled

**Corners** (relative to piece center):
- North: NW, NE, SW, SE
- East: NW, NE, SW, SE
- South: NW, NE, SW, SE
- West: NW, NE, SW, SE

**Types:**
- **Full T-Spin**: Both front corners (facing direction) filled
- **Mini T-Spin**: Only 3 corners filled, but not both front corners

## RNG

**7-Bag System:**
- Each "bag" contains one of each piece (I, O, T, S, Z, J, L)
- Bag is shuffled using deterministic LCG
- Pieces drawn sequentially from bag
- New bag generated when current is empty

**LCG Parameters:**
- a = 1664525
- c = 1013904223
- m = 2^32

## Game Actions

```rust
enum GameAction {
    MoveLeft,   // dx: -1, dy: 0
    MoveRight,  // dx: 1, dy: 0
    SoftDrop,   // dy: 1, score: +1 if successful
    HardDrop,   // Drop to bottom, score: +2 * cells
    RotateCw,   // Clockwise + SRS kicks
    RotateCcw,  // Counter-clockwise + SRS kicks
    Hold,       // Swap with hold piece
    Pause,      // Toggle pause state
    Restart,    // Reset game
}
```

## State Machine

```
┌──────────┐    Start     ┌─────────┐
│  Initial │ ───────────→ │ Playing │
└──────────┘              └────┬────┘
                               │
           ┌───────────────────┼───────────────────┐
           │                   │                   │
           ▼                   ▼                   ▼
    ┌──────────┐        ┌──────────┐       ┌──────────┐
    │ Paused   │←──────→│  Piece   │       │ GameOver │
    └──────────┘  P     │ Falling  │       └──────────┘
                        └────┬─────┘
                             │ Lock
                             ▼
                      ┌──────────┐
                      │  Locked  │
                      └────┬─────┘
                             │ Clear Lines
                             ▼
                      ┌──────────┐
                      │ Clearing │ (180ms pause)
                      └────┬─────┘
                             │ Spawn
                             ▼
                      Back to Playing
```

Notes:
- While paused, gameplay actions (move/rotate/drop/hold) are ignored; only `Pause` (toggle) and `Restart` are accepted.
- While game over, only `Restart` is accepted.

## References

- [Tetris Wiki - SRS](https://tetris.wiki/SRS)
- [Tetris Wiki - Scoring](https://tetris.wiki/Scoring)
- [Tetris Guideline](https://tetris.wiki/Tetris_Guideline)
