# Feature Matrix

Current: ruleset `guideline-ds-1.0.0`, adapter protocol 3.1.0.

- Core: 10×20, 7-bag, SRS (Y-down wiki kicks), hold, 500 ms / 15 lock, Guideline scoring
- Session: 16 ms fixed step, TTR2 replay
- Terminal: DAS/ARR, framebuffer diff flush, tetromino HUD, half-size hold/next previews, shared play/observe overlay, combo/B2B, pause/retry banners
- Adapter: TCP JSON-lines, controller/observer, causal `events` / acks
- Gates: workspace tests, clippy `-D warnings`, allocation gates, Criterion
