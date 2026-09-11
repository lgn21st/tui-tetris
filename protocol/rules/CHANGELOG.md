# Ruleset Changelog

## 1.1.0

Guideline-conformance corrections. Tables and scoring formulas are unchanged;
two classification/application rules now match the published Guideline, so
scores can differ from 1.0.0 and stored 1.0.0 replays are not expected to
re-verify.

- A lock that clears 0 lines is neutral for back-to-back, whether or not it is
  a T-Spin. 1.0.0 broke the chain on a 0-line non-T-Spin lock, which penalised
  the ordinary stacking that builds a Tetris or T-Spin.
- A T-Spin that would otherwise be Mini is scored as Full when the last
  successful rotation used the final SRS kick offset (the 1×2 kick).

Replay id: `guideline-ds-1.1.0`.

## 1.0.0

Portable snapshot (Tetris Guideline DS / Friends):

- SRS wall kicks, wiki Y-up; Y-down engines negate `dy`
- Full 7-bag, hold once per piece
- Lock delay 500 ms, 15 Extended Placement resets
- Line scores 100/300/500/800; T-Spin tables; B2B 1.5×
- Combo `50 * combo_index * guideline_level`
- Mini T-Spin with lines is B2B-capable
- 0-line T-Spin does not break B2B; combo still resets
- Engine level is 0-based (`lines / 10`); guideline level is `engine_level + 1`

Replay id: `guideline-ds-1.0.0`.
