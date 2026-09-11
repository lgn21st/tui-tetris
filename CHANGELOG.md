# Changelog

## [Unreleased]

- Gravity follows the Guideline curve to a normal-mode ceiling of 7 ms/row (≈2.4G) from level 15, instead of plateauing at 120 ms; soft drop clamps to at least 1 ms
- Local timing profile `tui-tetris-timing-1.1.0` is recorded with the portable ruleset as `<ruleset>+<timing>` in replay metadata, `welcome.ruleset_id`, and `diagnostic`
- Bounded-budget evaluation: `headless --budget-lines` / `--budget-pieces` and `replay eval` report a run's score at a fixed budget instead of relying on unbounded survival
- Adapter protocol `3.2.0`: `welcome` reports `ruleset_id` so clients can detect a ruleset change that leaves `protocol_version` unchanged
- Terminal HUD: tetromino previews, guideline level, combo/B2B, pause/game-over banners
- Play and observe share one five-line `HudOverlay`; the HUD next queue always shows five pieces
- Hold/next previews pack two mino rows per cell (`▀`/`▄`) so they stay square at half size
- Adapter status floats at the top-left; packed previews leave room for the full next queue
- Empty well is a flat fill; the per-cell `·` grid is gone
- Minos are 2×1 background fills so they stay square without `█` glyph distortion
- Mino cell size is chosen from measured terminal cell pixels (`TUI_TETRIS_CELL_W`/`H` override)
- Portable ruleset `guideline-ds-1.1.0`: a 0-line lock is neutral for back-to-back, and a Mini T-Spin reached with the final SRS 1×2 kick scores as Full
- SRS kicks negate wiki `dy` on this Y-down board
- Lock delay 500 ms / 15 Extended Placement resets
- Guideline DS/Friends scoring: 100/300/500/800, Mini-with-lines B2B, combo × guideline level
- A 0-line T-Spin no longer breaks back-to-back
- Replay id `guideline-ds-1.1.0`; adapter protocol 3.1.0 adds observation `combo` / `back_to_back`
- `GameState::take_last_event` is test-only; production drains `take_events`
- Adapter TCP line read/write lives in `framing`
