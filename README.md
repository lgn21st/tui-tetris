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

# Run a finite deterministic batch and exit
cargo run -- headless --seed 7 --steps 10000

# Record, verify, and inspect a replay
cargo run -- replay record /tmp/game.ttr --seed 7 --steps 1000
cargo run -- replay verify /tmp/game.ttr
cargo run -- replay inspect /tmp/game.ttr

# Print protocol/replay/ruleset diagnostics
cargo run -- diagnostic

# Observe a remote headless game over adapter TCP
cargo run -- observe --host 127.0.0.1 --port 7777

# Run without the TCP AI adapter (no listener)
TETRIS_AI_DISABLED=1 cargo run

# Run tests
cargo test --workspace
```

## Features

- ✅ Full Tetris rules: SRS rotation, T-Spin detection, B2B, combos
- ✅ 7-bag RNG: deterministic (seeded)
- ✅ Scoring: classic line clears, T-Spin tables, B2B, combos, soft/hard drop
- ✅ Full lifecycle: start, pause, game over, restart
- ✅ Ghost piece
- ✅ Hold
- ✅ AI control: current Tetris AI Adapter Protocol 3.0.0
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

See `docs/architecture.md` for enforced dependency boundaries and runtime data
flow.

## Project Layout

```
tui-tetris/
├── crates/
│   ├── tetris-core/              # deterministic rules and snapshots
│   ├── tetris-session/           # StepInput → Transition and Replay TTR2
│   ├── tetris-adapter-protocol/  # protocol v3 wire types
│   ├── tetris-adapter/           # broker, TCP transport, scheduling
│   └── tetris-terminal/          # InputCommand, GameViewModel, framebuffer
├── src/
│   ├── main.rs                   # composition root and runners
│   ├── observe.rs                # remote observer client
│   ├── replay_cli.rs             # replay commands
│   └── app_cli.rs                # headless/diagnostic commands
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
# Run all workspace tests (root package plus tetris-* crates)
cargo test --workspace

# Treat all Clippy findings as errors
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Crate-local suites
cargo test -p tetris-core
cargo test -p tetris-adapter --lib
```

Current status: `cargo test --workspace` passes.

Useful test suites:
- `cargo test --test adapter_acceptance_test`
- `cargo test --test adapter_e2e_test`
- `cargo test --test adapter_closed_loop_test`
- `cargo test --test no_alloc_gate_test`

## Roadmap

The maintained roadmap lives in `docs/roadmap.md`.

## Performance

Performance targets and optimization plan are tracked in `docs/roadmap.md`.
Benchmarks live under `benches/` and can be run via `cargo bench`.
The renderer pipeline benchmarks cover framebuffer rendering, diff encoding, and
buffer swapping. Backend benchmarks additionally cover `TerminalRenderer` calls
through `write_all` and `flush` using a deterministic injected writer. Host terminal
or PTY latency remains excluded because it is not suitable for a stable gate.

## Documentation

- Rules: `docs/rules-spec.md`
- Adapter documentation index: `docs/adapter.md`
- Current adapter protocol: `protocol/adapter/`
- tui-tetris adapter profile: `docs/adapter-tui-tetris.md`
- Roadmap: `docs/roadmap.md`
- Feature matrix: `docs/feature-matrix.md`
- Dev workflow: `AGENTS.md`
- Architecture and review notes: `docs/architecture.md`

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

## Observe Mode

Observe mode connects as an adapter observer client and renders remote observations locally.
This is useful for watching a headless game process.

Run:
```bash
cargo run -- observe --host 127.0.0.1 --port 7777
```

Notes:
- It requests `role=observer` and does not send game commands.
- It uses remote observations as source-of-truth (no local game tick simulation).
- If the target game restarts and the TCP connection drops, observe mode automatically retries reconnecting.
- If reconnect attempts are exhausted, observe mode exits gracefully.
- It overlays observe metadata in the top-left corner:
  - `MODE OBSERVE`
  - `TARGET <host:port>`
  - `STATE <WAITING|IDLE|PLAY|PAUSED|GAME_OVER>`
  - `EP <episode_id> PIECE <piece_id> STEP <step_in_piece>`
  - `SEED <seed>`
- Quit with `q` or `Ctrl+C`.

## HUD AI Metrics

AI panel metrics (runtime HUD):
- `CONN <n>`: current connected AI clients.
- `ST <code>`: adapter lifecycle status code.
- `CTRL <id|->`: active controller client id, or `-` when none.
- `PORT <p|->`: adapter listen port, or `-` when not listening.
- `PID <pid>`: current game process id.

`ST` code mapping:
- `0`: adapter off/disabled
- `1`: adapter listening, no clients
- `2`: clients connected, no controller
- `3`: controller assigned, no streaming observers
- `4`: streaming active

## Compatibility

The current shared protocol, JSON Schema, TCP profile, changelog, and conformance
client live in `protocol/adapter/`. tui-tetris runtime configuration and
implementation decisions are in `docs/adapter-tui-tetris.md`. With a local adapter
running, execute `python3 scripts/adapter_verify.py all`.

Environment variables:
- `TETRIS_AI_HOST` (default: `127.0.0.1`)
- `TETRIS_AI_PORT` (default: `7777`)
- `TETRIS_AI_DISABLED` (set to `1`/`true` to disable)
- `TETRIS_AI_OBS_HZ` (observation frequency in Hz; default: `20`; range: `1..60`)
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
