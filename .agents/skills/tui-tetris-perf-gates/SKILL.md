---
name: tui-tetris-perf-gates
description: Measure, optimize, and guard tui-tetris performance. Use for hot-path reviews, zero-allocation work, fixed-step/tick speed, snapshot or adapter serialization, terminal render/diff pipelines, Criterion benchmarks, regression diagnosis, or benchmark-threshold changes in /Users/daniel/workspace/learn/tui-tetris.
---

# TUI Tetris Performance Gates

Work from `/Users/daniel/workspace/learn/tui-tetris`. Preserve correctness and determinism before throughput.

## Identify the Performance Contract

- Core tick and snapshots: `crates/tetris-core`, `tests/no_alloc_gate_test.rs`
- Input update: `crates/tetris-terminal/src/input`, `tests/input_no_alloc_gate_test.rs`
- Adapter projection/serialization: `crates/tetris-adapter/src/adapter/observation.rs`, `tests/adapter_observation_no_alloc_gate_test.rs`; confirm whether the exercised snapshot changes board data or metadata only.
- Term view rendering: `crates/tetris-terminal/src/term`, `tests/term_no_alloc_gate_test.rs`
- Diff encoding: `crates/tetris-terminal/src/term/renderer.rs` and `tests/term_no_alloc_gate_test.rs` (`encode_diff_into`). Writer dispatch remains Criterion-covered.
- Integrated no-I/O path: `tests/e2e_no_alloc_gate_test.rs`
- Criterion coverage: `benches/game_logic.rs`
- Absolute regression thresholds: `scripts/bench_gate.py`

Read the benchmark body before trusting its name. Confirm setup is outside the timed region and that the benchmark exercises active representative state rather than an early return.

## Required Workflow

1. Define the target metric and correctness guardrail.
2. Inspect `git status`, the target code, its tests, benchmark, and allocation gate.
3. Add or fix the benchmark/no-allocation test before optimizing when coverage is missing.
4. Record a named baseline plus commit and environment metadata with the same toolchain, power state, command, and benchmark filter.
5. Make one scoped optimization without changing deterministic behavior.
6. Run correctness tests and allocation gates before measuring again.
7. Re-run the identical benchmark at least twice when results are noisy or surprising.
8. Run the absolute benchmark gate and inspect every failure.

Do not optimize by skipping required state updates, changing fixed-step timing, weakening protocol guarantees, or moving work outside a benchmark while leaving production unchanged.

## Run Allocation Gates

```bash
cargo test --test no_alloc_gate_test
cargo test --test input_no_alloc_gate_test
cargo test --test adapter_observation_no_alloc_gate_test
cargo test --test term_no_alloc_gate_test
cargo test --test e2e_no_alloc_gate_test
```

Warm reusable buffers before the measured allocation section. Keep device/network I/O outside deterministic no-allocation assertions unless a dedicated injected backend exists.

## Run Benchmarks

Capture enough context to reproduce a comparison:

```bash
git rev-parse HEAD
git status --short
rustc -Vv
cargo -V
uname -a
```

Record a unique named baseline before the change, then compare the identical target after it. Replace `<baseline>` with a name containing the commit or date and keep `<benchmark-filter>` unchanged:

```bash
cargo bench --bench game_logic -- <benchmark-filter> --save-baseline <baseline>
cargo bench --bench game_logic -- <benchmark-filter> --baseline <baseline>
```

Use a filter while iterating, then run the complete bench immediately before the absolute gate so every `new/estimates.json` consumed by the script comes from the current binary:

```bash
cargo bench --bench game_logic -- <benchmark-filter>
cargo bench --bench game_logic
python3 scripts/bench_gate.py
```

Criterion `target/` artifacts are evidence, not source files; do not commit them.

## Diagnose Regressions

- First confirm the changed code is reachable from the regressed benchmark.
- Require reachability from the diff, two repeatable identical target runs, and a statistically significant isolated change before calling a slowdown a code regression.
- Compare targeted results, confidence intervals, outliers, toolchain, power mode, thermal/load conditions, and sandbox versus unsandboxed scheduling.
- Treat simultaneous similar slowdowns across unrelated core, JSON, and renderer benchmarks as likely environment-wide until a code path proves otherwise.
- Treat a target-only repeatable slowdown as a code regression until explained.
- Re-run a single failing benchmark outside restrictive scheduling when permitted, but keep the command and binary profile identical.
- Never claim an improvement from one noisy run.

## Change Thresholds Carefully

Change `scripts/bench_gate.py` only when repeated measurements demonstrate that the existing threshold is invalid across supported environments or the benchmark contract intentionally changed.

Before raising a threshold:

1. Confirm no relevant production-code diff explains the failure.
2. Collect at least two stable measurements.
3. Check unrelated benchmarks for a machine-wide shift.
4. Set explicit cross-machine headroom and document the measured range beside the threshold.
5. Re-run `python3 scripts/bench_gate.py`.

Do not relax a threshold to hide a repeatable target-specific regression. Do not tighten one to a single machine's best result.

`scripts/bench_gate.py` checks absolute median point estimates only. It does not prove artifact freshness, commit/toolchain identity, statistical significance, or machine stability; preserve those facts separately in the handoff evidence.

## Handoff Evidence

Report the before/after median or range, whether Criterion found a statistically significant change, allocation-gate results, the absolute gate result, and any environment caveat. Run `cargo test`, Clippy, formatting, and `git diff --check` before committing benchmark or threshold changes.
