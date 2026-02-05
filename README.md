# TUI Tetris

A high-performance TUI Tetris game with external AI control support.

![Tetris](https://img.shields.io/badge/Rust-TUI-blue)
![Tests](https://img.shields.io/badge/Tests-passing-green)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Quick Start

```bash
# Clone
git clone <repository-url>
cd tui-tetris

# Run the game
cargo run

# Run in headless mode (no terminal UI; adapter-only loop)
TUI_TETRIS_HEADLESS=1 cargo run

# Run without the TCP AI adapter (no listener)
TETRIS_AI_DISABLED=1 cargo run

# Run tests
cargo test
```

## Features

- ✅ Full Tetris rules: SRS rotation, T-Spin detection, B2B, combos
- ✅ 7-bag RNG: deterministic (seeded)
- ✅ Scoring: classic line clears, T-Spin tables, B2B, combos, soft/hard drop
- ✅ Full lifecycle: start, pause, game over, restart
- ✅ Ghost piece
- ✅ Hold
- ✅ AI control: TCP protocol per `docs/adapter.md`
- ✅ DAS/ARR input (150ms / 50ms)
- ✅ Custom terminal renderer: framebuffer + diff flush (no ratatui widgets)

## Controls

| Key | Action |
|------|------|
| `← →` / `A D` / `h l` | Move left/right |
| `↑` / `W` | Rotate clockwise |
| `Z` / `Y` | Rotate counter-clockwise |
| `↓` / `S` / `j` | Soft drop |
| `Space` | Hard drop |
| `C` | Hold |
| `P` | Pause/resume |
| `R` | Restart |
| `Q` / `Ctrl+C` | Quit |

## Architecture

```
┌─────────┐  ┌─────────┐  ┌─────────┐
│   UI    │  │  Core   │  │ Adapter │
├─────────┤  ├─────────┤  ├─────────┤
│ Input   │  │ Board   │  │ Protocol│
│ Render  │←→│ Pieces  │←→│ (TCP)   │
│ Loop    │  │ RNG     │  │         │
│         │  │ Scoring │  │         │
│         │  │GameState│  │         │
└─────────┘  └─────────┘  └─────────┘
```

### Modules

- **Core**: deterministic and testable game logic
- **Input**: crossterm keyboard input + DAS/ARR
- **Term Renderer**: framebuffer + diff flush (terminal-native renderer)
- **Adapter**: AI protocol (JSON over TCP)

## Project Layout

```
tui-tetris/
├── src/
│   ├── main.rs           # entrypoint + main loop
│   ├── lib.rs            # crate exports
│   ├── types.rs          # shared types/constants
│   ├── input/            # terminal input (DAS/ARR)
│   │   ├── map.rs        # key mapping
│   │   └── handler.rs    # DAS/ARR handler
│   ├── core/             # game logic (no external deps)
│   │   ├── board.rs
│   │   ├── pieces.rs
│   │   ├── rng.rs
│   │   ├── scoring.rs
│   │   └── game_state.rs
│   ├── term/             # terminal rendering (framebuffer + diff flush)
│   │   ├── fb.rs
│   │   ├── game_view.rs
│   │   └── renderer.rs
│   ├── adapter/          # AI protocol server
│   │   ├── protocol.rs   # JSON messages
│   │   └── mod.rs
│   └── engine/           # reusable engine helpers
│       └── place.rs      # place-mode application logic
├── tests/                # integration tests
├── docs/                 # documentation
│   ├── rules-spec.md
│   ├── adapter.md
│   ├── roadmap.md
│   └── feature-matrix.md
└── Cargo.toml
```

## Testing

```bash
# Run all tests
cargo test

# Run specific tests
cargo test board
cargo test pieces
cargo test game_state

# Coverage (requires cargo-tarpaulin)
cargo tarpaulin --out Html
```

Current status: `cargo test` passes.

Useful test suites:
- `cargo test --test adapter_acceptance_test`
- `cargo test --test adapter_closed_loop_test`
- `cargo test --test no_alloc_gate_test`

## Roadmap

The maintained roadmap lives in `docs/roadmap.md`.

## Performance

Performance targets and optimization plan are tracked in `docs/roadmap.md`.
Benchmarks live under `benches/` and can be run via `cargo bench`.

## Documentation

- Rules: `docs/rules-spec.md`
- Adapter spec / acceptance gate: `docs/adapter.md`
- Roadmap: `docs/roadmap.md`
- Feature matrix: `docs/feature-matrix.md`
- Dev workflow: `AGENTS.md`

## Headless Mode

Headless mode runs the same deterministic game loop but **without** terminal setup, rendering, or keyboard input.
It is intended for automated integration and AI runs where the TCP adapter is the primary interface.

**Differences vs regular mode**
- No TUI rendering / no HUD.
- No local keyboard input (control happens via the adapter TCP protocol).
- Still runs fixed-timestep logic (`TICK_MS`) and applies adapter commands before ticking (determinism).

**How to start**
```bash
TUI_TETRIS_HEADLESS=1 cargo run
```

Optional knobs:
```bash
# Change observation frequency (Hz). Default: 20. Range: 1..60
TETRIS_AI_OBS_HZ=30 TUI_TETRIS_HEADLESS=1 cargo run

# Disable the adapter entirely (headless loop will run but will not listen)
TETRIS_AI_DISABLED=1 TUI_TETRIS_HEADLESS=1 cargo run
```

## Compatibility

AI protocol: see `docs/adapter.md`.

Environment variables:
- `TETRIS_AI_HOST` (default: `127.0.0.1`)
- `TETRIS_AI_PORT` (default: `7777`)
- `TETRIS_AI_DISABLED` (set to `1`/`true` to disable)
- `TETRIS_AI_OBS_HZ` (headless only; observation frequency in Hz; default: `20`; range: `1..60`)
- `TUI_TETRIS_HEADLESS` (set to `1`/`true`/`yes` to run without the terminal UI)
- `TUI_TETRIS_ANCHOR_Y` (optional; board vertical anchor: `top` or `center`; default: `center`)
- `TUI_TETRIS_KEY_RELEASE_TIMEOUT_MS` (input auto-release timeout for terminals without key release events; default: `150`)
  - Set `<150` for “tap moves once”; set `>150` to allow “hold repeats” on terminals without key-repeat events.
  - If your terminal emits key repeat events but not key release events, movement should stop shortly after repeats stop.
  - Some terminals report repeats as additional press events; the input handler treats those as repeat activity for repeat-driven auto-release.
- `TUI_TETRIS_REPEAT_RELEASE_TIMEOUT_MIN_MS` / `TUI_TETRIS_REPEAT_RELEASE_TIMEOUT_MAX_MS` (optional; repeat-driven release clamp for terminals with repeat-but-no-release)
  - Defaults: `80` / `300` (ms). Use this only if you need to tune repeat-driven stop behavior.

## Contributing

Follow the TDD workflow:

1. Write tests
2. Implement
3. Ensure tests pass
4. Commit

## License

This project is licensed under the [MIT License](https://opensource.org/licenses/MIT).

---

Made with ❤️ in Rust
