# Roadmap

This file is the current, maintained roadmap for tui-tetris.

## Current Status (2026-07-18)

- TUI runner: crossterm + custom framebuffer diff flush
- Core: deterministic, fixed-step tick (16ms), 7-bag RNG, scoring per `docs/rules-spec.md`
- Core snapshots: stable `GameSnapshot` + `GameState::snapshot_into` for adapter/render usage
- Input: DAS/ARR + timeout-based release for terminals without key-up events (`TUI_TETRIS_KEY_RELEASE_TIMEOUT_MS`), plus repeat-driven auto-release for terminals that emit repeats but not releases
- Adapter: TCP newline-delimited JSON protocol (protocol v3.0.0)
- Adapter performance: observation and line fanout avoid per-client clones (Arc-based fanout)
- Adapter runtime: avoids building/broadcasting observations when there are no streaming subscribers
- Runtime: interactive/headless modes share one deterministic `SessionRuntime`
  step and one reusable fixed-step backlog clock
- Local input: initial presses and DAS/ARR actions are queued to the same fixed-step boundary
- Snapshots/events: one coherent `SnapshotStore` and explicit `StepInput → Transition`
- Adapter bridge: direct bounded per-client replies and one latest-only typed
  broadcast observation slot
- Adapter broker: one client/controller state lock and one controller source of truth
- Adapter status: `client_count` and `controller_id` reflect live connections only
- Adapter I/O: buffered writes with a flush policy (immediate for welcome/ack/error; otherwise ≤16ms)
- Renderer I/O: injectable writer, diff-only flushes, and no write/flush for unchanged frames
- Performance gates: active-state tick plus render, diff, pipeline, and renderer-backend benchmarks
- Acceptance: automated protocol gates + closed-loop stability tests
- Replay: TTR2 ruleset-versioned tapes, complete transition hash verification,
  first mismatch, minimal prefixes, and record/verify/inspect CLI
- Workspace: physically owned `tetris-core`, `tetris-session`,
  `tetris-adapter-protocol`, `tetris-adapter`, and `tetris-terminal` packages
- CLI: interactive, real-time headless, finite deterministic headless, replay,
  observe, and diagnostic modes
- Stress: disconnect storms, reliable-output flooding, and 32-observer fanout

## Maintenance Priorities

1) Performance end-to-end
- Keep all allocation gates green and investigate any newly observed hot-path
  allocation before accepting it.
- Add/maintain benchmarks (`cargo bench`, see `benches/`) and regression thresholds

2) Protocol polish
- Expand schema/docs as new fields/behaviors are added; keep docs in sync with implementation
- Documentation default language: English
- Use explicit major versions for intentional wire breaks and maintain
  deterministic command ordering

3) Core API hardening
- Reduce `pub` surface of `GameState` where practical
- Keep snapshot APIs allocation-free and stable for adapter/render use

4) Structural decomposition
- ✅ Replace duplicate interactive/headless authoritative step bodies
- ✅ Queue all local gameplay actions to fixed-step boundaries
- ✅ Centralize coherent snapshot ownership and explicit step results
- ✅ Replace production unbounded outbound delivery
- ✅ Consolidate controller/client state into one broker state
- ✅ Canonicalize state hashing independently of Rust `Hash`
- ✅ Remove duplicate outbound compatibility variants and copied engine harnesses
- ✅ Split core, session, protocol, adapter, and terminal into source-owning packages
- ✅ Extract shared command draining and observation scheduling from interactive/headless runners
- ✅ Separate `GameState` unit tests from the production implementation file
- ✅ Separate adapter observation projection/state hashing and server configuration from TCP lifecycle
- ✅ Share adapter TCP line/server fixtures across integration suites
- Continue splitting connection framing and writer code when those flows change;
  the correctness-sensitive broker state is already consolidated
- Keep protocol versions and other cross-process compatibility constants centralized

## Validation Checklist

- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`
- Adapter acceptance: `cargo test --test adapter_acceptance_test`
- Adapter e2e: `cargo test --test adapter_e2e_test`
- Closed-loop stability: `cargo test --test adapter_closed_loop_test`
- Long-run gate (optional): `cargo test --test adapter_closed_loop_test -- --ignored`
- Core allocation gate: `cargo test --test no_alloc_gate_test`
- Input allocation gate: `cargo test --test input_no_alloc_gate_test`
- Adapter observation allocation gate: `cargo test --test adapter_observation_no_alloc_gate_test`
- Term render allocation gate: `cargo test --test term_no_alloc_gate_test`
- End-to-end allocation gate (no I/O): `cargo test --test e2e_no_alloc_gate_test`
- Benchmarks: `cargo bench`
- Benchmark regression gate: `python3 scripts/bench_gate.py`
