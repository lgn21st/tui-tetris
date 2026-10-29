# Changelog

## [Unreleased]

- Playfield and HUD previews are drawn with layered Kitty graphics instead of character cells (`docs/kitty-graphics.md`); the game refuses to start without a pixel backend
- Layers split by change frequency: opaque board (`z=0`, RGB) on a lock or clear, transparent motion (`z=1`, RGBA) per piece movement, plus native-size HOLD/NEXT sprites at half a board mino
- The frame hugs the pixel grid, so minos are exact squares (19 text rows for 20 minos on an 8x17 cell, image sent at device resolution); cells fill their band edge to edge and one painter covers falling and locked pieces
- Motion-layer effects: drop shadow, translucent ghost silhouette, lock-delay whitening, line-clear flash, pause dim; the side panel keeps the cell renderer's composition
- Gravity follows the Guideline curve to a normal-mode ceiling of 7 ms/row (≈2.4G) from level 15, instead of plateauing at 120 ms; soft drop clamps to at least 1 ms
- Local timing profile `tui-tetris-timing-1.1.0` is recorded with the portable ruleset as `<ruleset>+<timing>` in replay metadata, `welcome.ruleset_id`, and `diagnostic`
- Bounded-budget evaluation: `headless --budget-lines` / `--budget-pieces` and `replay eval` report a run's score at a fixed budget instead of relying on unbounded survival
- Adapter protocol `3.2.0`: `welcome` reports `ruleset_id` so clients can detect a ruleset change that leaves `protocol_version` unchanged
- Play and observe share one five-line `HudOverlay`; the HUD next queue always shows five pieces and the adapter status floats at the top-left
- Portable ruleset `guideline-ds-1.1.0`: 0-line locks are B2B neutral, a Mini T-Spin from the final SRS 1×2 kick scores as Full, and a 0-line T-Spin does not break back-to-back
- SRS kicks negate wiki `dy` on this Y-down board; lock delay 500 ms / 15 Extended Placement resets
- Guideline DS/Friends scoring: 100/300/500/800, Mini-with-lines B2B, combo × guideline level
- Replay id `guideline-ds-1.1.0`; adapter protocol 3.1.0 adds observation `combo` / `back_to_back`
- `GameState::take_last_event` is test-only; production drains `take_events`
- Adapter TCP line read/write lives in `framing`
