# Roadmap

This file is the current, maintained roadmap for tui-tetris.

## Current Status (2026-02-03)

- TUI runner: crossterm + custom framebuffer diff flush
- Core: deterministic, fixed-step tick (16ms), 7-bag RNG, scoring aligned with swiftui-tetris
- Core snapshots: stable `GameSnapshot` + `GameState::snapshot_into` for adapter/render usage
- Input: DAS/ARR + timeout-based release for terminals without key-up events
- Adapter: TCP newline-delimited JSON protocol compatible with swiftui-tetris (protocol v2.0.0 + schema gate)
- Adapter performance: observation and line fanout avoid per-client clones (Arc-based fanout)
- Adapter runtime: avoids building/broadcasting observations when there are no streaming subscribers
- Adapter status: `client_count` and `controller_id` reflect live connections only
- Adapter I/O: buffered writes with a flush policy (immediate for welcome/ack/error; otherwise ≤16ms)
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

## Validation Checklist

- `cargo test`
- Schema gate: `cargo test --test schema_gate_test`
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
