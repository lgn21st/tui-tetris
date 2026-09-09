# AGENTS

## Project Intent
- Build a high-performance TUI Tetris game in Rust with external AI control support
- Keep Core deterministic and testable; rendering/adapter stay decoupled
- Strict TDD: every feature/improvement/refactor must add/adjust tests first

## Key Docs
- `protocol/rules/SPEC.md`: portable Guideline DS/Friends rules
- `docs/rules-spec.md`: tui-tetris local timing/input profile
- `docs/feature-matrix.md`: current capability snapshot
- `docs/roadmap.md`: remaining work
- `docs/architecture.md`: crate boundaries and runtime flow
- `docs/development-workflow.md`: change order and local validation
- `protocol/adapter/SPEC.md`: current, implementation-neutral AI protocol
- `protocol/adapter/schema.json`: machine-readable protocol schema
- `docs/adapter.md`: adapter documentation index
- `docs/adapter-tui-tetris.md`: tui-tetris implementation profile

## Architecture Expectations
- `tetris-core` owns board, pieces, RNG, scoring, timing, snapshots, and actions. It may use container crates such as `arrayvec`, but not terminal, network, serde, or async dependencies
- `tetris-session` owns `SessionRuntime`, fixed-step accounting, atomic place, and replay TTR2
- `tetris-adapter-protocol` owns wire types; `tetris-adapter` owns TCP, broker, mailboxes, and scheduling
- `tetris-terminal` is crossterm-only: input mapping/DAS/ARR plus framebuffer diff flush (no ratatui widgets)
- Root `tui-tetris` is composition/CLI only (`main`, observe, replay, diagnostic). Import APIs from the owning crate; do not reexport dependency layers
- Rendering uses diff-based updates for performance (dirty-cells / dirty-rects)

## Working Agreements
- Follow strict TDD: write tests first, then implement
- Core changes first; UI changes come after logic is stable
- If portable rules change, update `protocol/rules/` and `docs/feature-matrix.md`
- If only local timing/input changes, update `docs/rules-spec.md` and `docs/feature-matrix.md`
- Zero-allocation in hot paths (tick, render, observation build/serialize, diff encode)
- Fixed timestep: 16ms logic updates

## Testing Strategy
- Core tests: rule compliance, timing, edge cases
- Adapter tests: protocol parsing, connection handling, backpressure, closed-loop
- Renderer tests: snapshot-style framebuffer tests for critical paths
- Run `cargo test --workspace` before every commit

## Dependencies
- Core: pure Rust plus container crates (`arrayvec`); no terminal/network/serde/async
- Adapter: tokio, serde, serde_json (async networking)
- Terminal: crossterm (I/O), custom framebuffer renderer (no ratatui)

## Protocol Compatibility
- AI protocol 100% compatible with `protocol/adapter/SPEC.md` and its selected TCP profile
- Same environment variables: `TETRIS_AI_HOST`, `TETRIS_AI_PORT`, `TETRIS_AI_DISABLED`
- Same JSON message format and error codes

## Validate

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```
