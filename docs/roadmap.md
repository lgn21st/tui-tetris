# Roadmap

This file is the current, maintained roadmap for tui-tetris.

## Current Status (2026-07-15)

- TUI runner: crossterm + custom framebuffer diff flush
- Core: deterministic, fixed-step tick (16ms), 7-bag RNG, scoring per `docs/rules-spec.md`
- Core snapshots: stable `GameSnapshot` + `GameState::snapshot_into` for adapter/render usage
- Input: DAS/ARR + timeout-based release for terminals without key-up events (`TUI_TETRIS_KEY_RELEASE_TIMEOUT_MS`), plus repeat-driven auto-release for terminals that emit repeats but not releases
- Adapter: TCP newline-delimited JSON protocol (protocol v2.1.1)
- Adapter performance: observation and line fanout avoid per-client clones (Arc-based fanout)
- Adapter runtime: avoids building/broadcasting observations when there are no streaming subscribers
- Adapter runners: interactive/headless modes share adapter-owned bounded command draining and observation scheduling
- Adapter status: `client_count` and `controller_id` reflect live connections only
- Adapter I/O: buffered writes with a flush policy (immediate for welcome/ack/error; otherwise ≤16ms)
- Renderer I/O: injectable writer, diff-only flushes, and no write/flush for unchanged frames
- Performance gates: active-state tick plus render, diff, pipeline, and renderer-backend benchmarks
- Acceptance: automated protocol gates + closed-loop stability tests

## Next Priorities

1) Performance end-to-end
- Keep all allocation gate tests green; remove remaining per-frame allocations outside core (input + adapter observation build/serialization)
- Add/maintain benchmarks (`cargo bench`, see `benches/`) and regression thresholds

2) Protocol polish
- Expand schema/docs as new fields/behaviors are added; keep docs in sync with implementation
- Documentation default language: English
- Maintain strict compatibility and deterministic command ordering

3) Core API hardening
- Reduce `pub` surface of `GameState` where practical
- Keep snapshot APIs allocation-free and stable for adapter/render use

4) Structural decomposition
- ✅ Extract shared command draining and observation scheduling from interactive/headless runners
- ✅ Separate `GameState` unit tests from the production implementation file
- ✅ Separate adapter observation projection/state hashing and server configuration from TCP lifecycle
- ✅ Share adapter TCP line/server fixtures across integration suites
- Continue splitting connection, control-policy, and writer responsibilities when those flows change
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
