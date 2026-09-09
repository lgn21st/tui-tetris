# Ruleset Changelog

## 1.0.0

Current portable snapshot (Tetris Guideline DS / Friends):

- SRS wall kicks, wiki Y-up; Y-down engines negate `dy`
- Full 7-bag, hold once per piece
- Lock delay 500 ms, 15 Extended Placement resets
- Line scores 100/300/500/800; T-Spin tables; B2B 1.5×
- Combo `50 * combo_index * guideline_level`
- Mini T-Spin with lines is B2B-capable
- 0-line T-Spin does not break B2B; combo still resets
- Engine level is 0-based (`lines / 10`); guideline level is `engine_level + 1`

Replay id: `guideline-ds-1.0.0`.
