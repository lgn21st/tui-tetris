# Roadmap

This file is the current, maintained roadmap for tui-tetris.

## Current Status (2026-02-02)

- TUI runner: crossterm + custom framebuffer diff flush
- Core: deterministic, fixed-step tick (16ms), 7-bag RNG, scoring, timing
- Input: DAS/ARR + timeout-based release for terminals without key-up events
- Adapter: TCP newline-delimited JSON protocol compatible with swiftui-tetris
- Acceptance: automated protocol gates + closed-loop stability tests

## Next Priorities

1) Performance end-to-end
- Remove remaining per-frame allocations outside core (input + adapter observation build)
- Add/maintain benchmarks (`cargo bench`) and regression thresholds

2) Protocol polish
- Expand schema/docs as new fields/behaviors are added
- Maintain strict compatibility and deterministic command ordering

3) Core API hardening
- Reduce `pub` surface of `GameState` where practical
- Add explicit snapshot structs for adapter/render use

## Validation Checklist

- `cargo test`
- Adapter acceptance: `cargo test --test adapter_acceptance_test`
- Closed-loop stability: `cargo test --test adapter_closed_loop_test`
- Long-run gate (optional): `cargo test --test adapter_closed_loop_test -- --ignored`
- Core allocation gate: `cargo test --test no_alloc_gate_test`
- Input allocation gate: `cargo test --test input_no_alloc_gate_test`
- Adapter observation allocation gate: `cargo test --test adapter_observation_no_alloc_gate_test`
- Term render allocation gate: `cargo test --test term_no_alloc_gate_test`
- End-to-end allocation gate (no I/O): `cargo test --test e2e_no_alloc_gate_test`
