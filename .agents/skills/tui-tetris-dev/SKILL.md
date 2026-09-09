---
name: tui-tetris-dev
description: Develop, refactor, review, diagnose, and document the Rust tui-tetris project. Use when Codex changes or audits gameplay rules, fixed-step loops, engine placement, terminal rendering, input/DAS/ARR, adapter integration, architecture, tests, or project documentation in /Users/daniel/workspace/learn/tui-tetris.
---

# TUI Tetris Development

Work from `/Users/daniel/workspace/learn/tui-tetris`.

## Start Safely

1. Read `AGENTS.md` completely.
2. Inspect `git status --short --branch`; preserve unrelated user changes.
3. Read only the source-of-truth documents relevant to the task:
   - Portable rules: `protocol/rules/SPEC.md`
   - Local timing/input profile: `docs/rules-spec.md`
   - Adapter behavior: `docs/adapter.md`
   - Architecture boundaries: `docs/architecture.md`
   - Completion/status claims: `docs/feature-matrix.md` and `docs/roadmap.md`
4. Locate code and tests with `rg` or `rg --files` before editing.

## Required TDD Workflow

1. State the behavior or invariant being changed.
2. Add or adjust the smallest test that should fail before production code changes.
3. Run that test and confirm it fails for the intended reason.
4. Implement the minimal scoped change.
5. Re-run the targeted test, then the affected subsystem suite.
6. Synchronize behavior, architecture, feature, and changelog documentation.
7. Run the full validation required for the changed risk surface.

Do not weaken assertions merely to make a test pass. For a pure documentation correction, update a documentation consistency test when the contract is machine-checkable; otherwise validate all examples, commands, and links directly.

## Route Changes by Ownership

- `crates/tetris-core`: deterministic board, pieces, RNG, scoring, timing, actions, snapshots; keep free of terminal/network/serde/async dependencies.
- `crates/tetris-session`: command application and atomic high-level placement over core state.
- `crates/tetris-terminal` input: key mapping, DAS/ARR, repeat and key-release emulation.
- `crates/tetris-terminal` term: framebuffer, layout, render throttling, diff encoding, terminal writer boundary.
- `crates/tetris-adapter` / `crates/tetris-adapter-protocol`: protocol, control lifecycle, queues, observation projection/scheduling, TCP and runtime startup.
- `src/main.rs`: composition and fixed-step runners only; do not move rule logic here.
- `src/observe.rs`: remote observation client and presentation, not authoritative game state.

Change core behavior before adapting UI or protocol projections. Cross boundaries only for a necessary typed interface change.

## Preserve Runtime Invariants

- Keep core deterministic for equal seed, command sequence, and fixed-step timing.
- Keep logic steps at `TICK_MS = 16`; retain elapsed backlog and process at most eight catch-up steps per outer-loop iteration.
- Preserve adapter step ordering: drain/apply commands, tick rules, then emit observations.
- Keep `tick`, snapshot projection, input update, render, and other declared hot paths allocation-free after warm-up.
- Keep rendering snapshot-driven and diff-based; never mutate game rules from terminal code.
- Use saturating or explicitly checked arithmetic where long-running counters can overflow.
- Preserve atomic failure semantics for high-level commands.

For adapter protocol, lifecycle, concurrency, or backpressure work, also use `tui-tetris-adapter-qa`. For hot-path or benchmark work, also use `tui-tetris-perf-gates`.

## Synchronize Documentation

- Portable rule behavior: update `protocol/rules/` and `docs/feature-matrix.md`.
- Local timing or input behavior: update `docs/rules-spec.md` and `docs/feature-matrix.md`.
- Protocol fields or observable adapter semantics: update `docs/adapter.md`, examples/schema, and `docs/feature-matrix.md`.
- Module ownership or runtime flow: update `docs/architecture.md`; update `docs/roadmap.md` only when current status or priorities change.
- User-visible or notable technical behavior: update `CHANGELOG.md`.

## Validate

Run targeted tests first, then before every commit run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Useful targeted suites:

```bash
cargo test -p tetris-core
cargo test -p tetris-terminal --lib
cargo test --test term_game_view_test
cargo test --test adapter_acceptance_test
cargo test --test adapter_e2e_test
```

If TCP tests fail only because localhost binding is forbidden, rerun them in an environment that permits loopback sockets; do not reinterpret the permission error as a protocol failure.
