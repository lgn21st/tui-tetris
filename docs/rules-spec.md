# Tetris Rules Specification

Comprehensive rules and timing constants for tui-tetris.
Matches swiftui-tetris for AI compatibility.

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

*... (S, Z, J, L pieces similar)*

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
0→1: [(0,0), (-2,0), (1,0), (-2,-1), (1,2)]
0→3: [(0,0), (-1,0), (2,0), (-1,2), (2,-1)]
...
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

### DAS/ARR (Not yet implemented)

- **DAS (Delayed Auto Shift)**: 150ms
- **ARR (Auto Repeat Rate)**: 50ms

## Scoring

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

### Combo Bonus

`combo_score = 50 * combo_count * (level + 1)`

### Back-to-Back

- **Qualifies**: T-spin full with lines OR Tetris (4 lines)
- **Bonus**: 1.5× total score (applies to next qualifying clear)

### Drop Scoring

- **Soft Drop**: +1 per cell
- **Hard Drop**: +2 per cell

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

## References

- [Tetris Wiki - SRS](https://tetris.wiki/SRS)
- [Tetris Wiki - Scoring](https://tetris.wiki/Scoring)
- [Tetris Guideline](https://tetris.wiki/Tetris_Guideline)
