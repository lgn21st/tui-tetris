# Tetris Guideline DS/Friends Ruleset 1.0.0

This document is the normative, implementation-neutral contract for the
portable ruleset frozen in this directory. The key words MUST, MUST NOT,
SHOULD, SHOULD NOT, and MAY are normative.

Machine-readable tables live in `constants.json`. Kick offsets in this
package use wiki convention: **+x right, +y opposite gravity (up)**.

This snapshot documents Tetris Guideline behavior as recorded for
**Tetris DS / Tetris Friends** era games. It is not a TTC certification
suite.

## 1. Versioning

- The ruleset version is semantic version text in `VERSION`.
- Implementations MUST record the replay metadata id `guideline-ds-1.0.0`
  for this release.
- Patch releases preserve tables and scoring formulas.
- This package is independent of the AI adapter protocol in
  `protocol/adapter/`.

## 2. Playfield and spawn

- Visible playfield MUST be 10 columns by 20 rows.
- Column 0 is the left edge. Row 0 is the spawn edge (opposite gravity).
- Gravity MUST move pieces toward increasing row index on a Y-down board,
  or toward decreasing y on a Y-up board.
- Each tetromino MUST spawn at piece origin `(3, 0)` on the visible field
  in rotation North.
- Hidden/vanishing rows, 20G gravity, ARE/line delay, Perfect Clear, DAS/ARR,
  and Infinity lock reset are out of scope for this version.

## 3. Pieces

Mino offsets are piece-local: **+x right, +y in the gravity direction**.
Y-down engines MAY store these offsets unchanged.

### I

- North: `(0,1), (1,1), (2,1), (3,1)`
- East: `(2,0), (2,1), (2,2), (2,3)`
- South: `(0,2), (1,2), (2,2), (3,2)`
- West: `(1,0), (1,1), (1,2), (1,3)`

### O

- All rotations: `(1,0), (2,0), (1,1), (2,1)`

### T

- North: `(1,0), (0,1), (1,1), (2,1)`
- East: `(1,0), (1,1), (2,1), (1,2)`
- South: `(0,1), (1,1), (2,1), (1,2)`
- West: `(1,0), (0,1), (1,1), (1,2)`

### S

- North: `(1,0), (2,0), (0,1), (1,1)`
- East: `(1,0), (1,1), (2,1), (2,2)`
- South: `(1,1), (2,1), (0,2), (1,2)`
- West: `(0,0), (0,1), (1,1), (1,2)`

### Z

- North: `(0,0), (1,0), (1,1), (2,1)`
- East: `(2,0), (1,1), (2,1), (1,2)`
- South: `(0,1), (1,1), (1,2), (2,2)`
- West: `(1,0), (0,1), (1,1), (0,2)`

### J

- North: `(0,0), (0,1), (1,1), (2,1)`
- East: `(1,0), (2,0), (1,1), (1,2)`
- South: `(0,1), (1,1), (2,1), (2,2)`
- West: `(1,0), (1,1), (0,2), (1,2)`

### L

- North: `(2,0), (0,1), (1,1), (2,1)`
- East: `(1,0), (1,1), (1,2), (2,2)`
- South: `(0,1), (1,1), (2,1), (0,2)`
- West: `(0,0), (1,0), (1,1), (1,2)`

## 4. Rotation (SRS)

- Pieces MUST rotate with the Super Rotation System.
- Kick tables MUST be tried in listed order, including the `(0,0)` test.
- The O piece kick table is `[(0,0)]`. Its rotation index still advances.
- Playfields whose y increases toward gravity MUST negate kick `dy` and
  MUST NOT negate `dx`.

### JLSTZ kicks (wiki Y-up)

```
N→E: (0,0), (-1,0), (-1,+1), (0,-2), (-1,-2)
N→W: (0,0), (+1,0), (+1,+1), (0,-2), (+1,-2)
E→N: (0,0), (+1,0), (+1,-1), (0,+2), (+1,+2)
E→S: (0,0), (+1,0), (+1,-1), (0,+2), (+1,+2)
S→E: (0,0), (-1,0), (-1,+1), (0,-2), (-1,-2)
S→W: (0,0), (+1,0), (+1,+1), (0,-2), (+1,-2)
W→S: (0,0), (-1,0), (-1,-1), (0,+2), (-1,+2)
W→N: (0,0), (-1,0), (-1,-1), (0,+2), (-1,+2)
```

### I kicks (wiki Y-up)

```
N→E: (0,0), (-2,0), (+1,0), (-2,-1), (+1,+2)
N→W: (0,0), (-1,0), (+2,0), (-1,+2), (+2,-1)
E→N: (0,0), (+2,0), (-1,0), (+2,+1), (-1,-2)
E→S: (0,0), (-1,0), (+2,0), (-1,+2), (+2,-1)
S→E: (0,0), (+1,0), (-2,0), (+1,-2), (-2,+1)
S→W: (0,0), (+2,0), (-1,0), (+2,+1), (-1,-2)
W→S: (0,0), (-2,0), (+1,0), (-2,-1), (+1,+2)
W→N: (0,0), (+1,0), (-2,0), (+1,-2), (-2,+1)
```

## 5. Randomizer and hold

- The randomizer MUST be a full 7-bag: each bag contains I, O, T, S, Z, J,
  and L exactly once, then a new bag is generated.
- Shuffle algorithm is implementation-defined provided the bag contents
  remain complete and unbiased across bags.
- Hold MAY store one piece. A piece MUST NOT be held more than once before
  the next lock.

## 6. Lock delay

- When a piece is grounded, lock delay MUST be 500 ms.
- Successful moves or rotations that leave the piece grounded MUST reset
  the lock timer and consume one Extended Placement reset.
- Each piece MUST receive at most 15 lock-timer resets.
- After the reset limit, the lock timer MUST continue without further
  resets until lock or the piece becomes airborne.
- Infinity (unlimited resets) MUST NOT be used.

## 7. Level

- Engine level MUST be `floor(lines / 10)` and MAY be 0-based.
- Guideline level MUST be `engine_level + 1` (DS HUD starts at 1).
- Scoring formulas in this document use guideline level unless stated
  otherwise.

## 8. Scoring

Scoring MUST use the **pre-clear** engine level (converted to guideline
level) from before newly cleared lines are added.

### Line clears

| Lines | Base |
|-------|------|
| 1 | 100 |
| 2 | 300 |
| 3 | 500 |
| 4 | 800 |

`line_score = base * guideline_level`

### T-Spin

| Kind | Lines | Base |
|------|-------|------|
| Full | 0 | 400 |
| Full | 1 | 800 |
| Full | 2 | 1200 |
| Full | 3 | 1600 |
| Mini | 0 | 100 |
| Mini | 1 | 200 |
| Mini | 2 | 400 |

When a T-Spin is detected, the T-Spin table MUST be used instead of the
line-clear table. The two MUST NOT be added together.

### Back-to-back

A clear QUALIFIES when:

- T-Spin Full with 1 through 4 lines, or
- T-Spin Mini with 1 through 4 lines, or
- Tetris (4 lines, no T-Spin).

A consecutive qualifying clear MUST multiply the base clear points by
`3/2` before combo is added.

A T-Spin (Full or Mini) that locks with 0 lines MUST NOT break an existing
back-to-back chain and MUST NOT start one.

A lock with 0 lines and no T-Spin MUST reset the back-to-back chain.

### Combo

- `combo_index` is `-1` when no chain is active.
- The first line-clearing lock in a chain uses `combo_index = 0` and awards
  no combo bonus.
- Each consecutive line-clearing lock increments `combo_index` by 1.
- `combo_bonus = 50 * combo_index * guideline_level` when `combo_index > 0`,
  otherwise 0.
- Combo bonus is added after the back-to-back multiplier.
- Any lock with 0 lines MUST reset `combo_index` to `-1`, including a
  0-line T-Spin.

### Drop

- Soft drop: +1 per cell moved by an explicit soft-drop action.
- Hard drop: +2 per cell.

## 9. T-Spin detection

A lock is a T-Spin when all of the following hold:

1. The locked piece is T.
2. The last successful action was a rotation.
3. At least 3 of the 4 corners around the T center are occupied or out of
   bounds.

Corner occupancy MUST be evaluated after the piece locks and before
completed rows are removed.

- **Full**: both front corners (facing the T stem) are occupied.
- **Mini**: three corners occupied, but not both front corners.

Immobile T locks that were not preceded by a rotation MUST NOT score as a
T-Spin.

## 10. Out of scope for 1.0.0

Implementations MAY choose local values for gravity tables, DAS/ARR,
line-clear animation pause, landing flash, tick length, next-queue length,
and whether 0-line T-Spins appear in observation events. Those choices MUST
NOT be copied into this package.
