# Changelog

## [Unreleased]

- Terminal HUD: tetromino previews, guideline level, combo/B2B, pause/game-over banners
- Play and observe share one five-line `HudOverlay`; the HUD next queue always shows five pieces
- Hold/next previews pack two mino rows per cell (`▀`/`▄`) so they stay square at half size
- Adapter status floats at the top-left; packed previews leave room for the full next queue
- Empty well is a flat fill; the per-cell `·` grid is gone
- Minos are 2×1 background fills so they stay square without `█` glyph distortion
- Mino cell size is chosen from measured terminal cell pixels (`TUI_TETRIS_CELL_W`/`H` override)
- Portable ruleset `guideline-ds-1.0.0` in `protocol/rules/`
- SRS kicks negate wiki `dy` on this Y-down board
- Lock delay 500 ms / 15 Extended Placement resets
- Guideline DS/Friends scoring: 100/300/500/800, Mini-with-lines B2B, combo × guideline level
- A 0-line T-Spin no longer breaks back-to-back
- Replay id `guideline-ds-1.0.0`; adapter protocol remains 3.0.0
