# TUI Tetris

High-performance TUI Tetris with external AI control.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Quick Start

```bash
cargo run
TUI_TETRIS_HEADLESS=1 cargo run
cargo run -- headless --seed 7 --steps 10000
cargo run -- replay record /tmp/game.ttr --seed 7 --steps 1000
cargo run -- observe --host 127.0.0.1 --port 7777
TETRIS_AI_DISABLED=1 cargo run
cargo test --workspace
```

## Controls

| Key | Action |
|------|------|
| `← →` / `A D` / `h l` | Move |
| `↑` / `W` | Rotate CW |
| `Z` / `Y` | Rotate CCW |
| `↓` / `S` / `j` | Soft drop |
| `Space` | Hard drop |
| `C` | Hold |
| `P` | Pause |
| `R` | Restart |
| `Q` / `Ctrl+C` | Quit |

## Docs

- Portable rules: `protocol/rules/`
- Local timing/input: `docs/rules-spec.md`
- Adapter protocol: `protocol/adapter/`
- Local adapter profile: `docs/adapter-tui-tetris.md`
- Architecture: `docs/architecture.md`
- Workflow: `AGENTS.md`

## Layout

```
crates/tetris-{core,session,adapter-protocol,adapter,terminal}
protocol/{adapter,rules}
src/{main,observe,replay_cli,app_cli}.rs
```

## License

[MIT](https://opensource.org/licenses/MIT)
