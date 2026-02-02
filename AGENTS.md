# AGENTS

## Project Intent
- Build a high-performance TUI Tetris game in Rust with external AI control support
- Match gameplay rules and timing with swiftui-tetris for AI compatibility
- Keep Core deterministic and testable; rendering/adapter stay decoupled
- Strict TDD: every feature/improvement/refactor must add/adjust tests first

## Key Docs
- `docs/rules-spec.md`: authoritative rules/timing constants (mirrors swiftui-tetris)
- `docs/feature-matrix.md`: feature checklist
- `docs/roadmap.md`: goals and validation checklist
- `docs/adapter-protocol.md`: AI communication protocol specification

## Architecture Expectations
- `core` owns board, pieces, RNG, scoring, timing, and actions (NO external deps)
- `adapter` handles AI protocol (TCP socket, JSON line protocol)
- `term` is crossterm-only: terminal framebuffer + renderer flush (no ratatui widgets)
- `input` handles key mapping and DAS/ARR (works with terminals without key-release events)
- Rendering should use diff-based updates for performance (dirty-cells / dirty-rects)

## Working Agreements
- Follow strict TDD: write tests first, then implement
- Core changes first; UI changes come after logic is stable
- If behavior changes, update `docs/rules-spec.md` and `docs/feature-matrix.md`
- Zero-allocation in hot paths (tick, render)
- Fixed timestep: 16ms logic updates

## Testing Strategy
- Core tests: rule compliance, timing, edge cases (>90% coverage)
- Adapter tests: protocol parsing, connection handling (>80% coverage)
- Renderer tests: snapshot-style framebuffer tests for critical paths
- Run `cargo test` before every commit

## Dependencies
- Core: pure Rust, no std library dependencies beyond containers
- Adapter: tokio, serde, serde_json (async networking)
- Terminal: crossterm (I/O), custom framebuffer renderer (no ratatui)

## Protocol Compatibility
- AI protocol 100% compatible with swiftui-tetris
- Same environment variables: `TETRIS_AI_HOST`, `TETRIS_AI_PORT`, `TETRIS_AI_DISABLED`
- Same JSON message format and error codes
