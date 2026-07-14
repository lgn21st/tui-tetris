# Architecture

This document describes the intended dependency boundaries and the current
runtime flows. Rule details belong in `rules-spec.md`; wire compatibility belongs
in `adapter.md`.

## Dependency Boundaries

```text
main / observe
  |-- input ----> types
  |-- term -----> core snapshots, types
  |-- adapter --> core snapshots, types
  |-- engine ---> core, types
  `-- core -----> types
```

- `core` owns deterministic game state, rules, scoring, timing, RNG, and snapshots.
  It must not depend on terminal or network code.
- `input` translates terminal key state into `GameAction` values. DAS/ARR state
  stays here rather than in the game rules.
- `term` renders immutable snapshots through a framebuffer and diff flush. It must
  not mutate `GameState`.
- `adapter` owns JSON-line protocol types, TCP lifecycle, control policy, and
  backpressure. `ObservationSchedule` owns cadence, critical-state detection, and
  sequence generation shared by interactive and headless modes. Its `game_loop`
  boundary owns bounded pre-tick command draining, snapshot requests, and
  acknowledgements. Game commands cross the boundary as typed messages.
- `main` is the composition root. Interactive, headless, and observe modes select
  which input/output edges are active.

## Fixed-Step Runtime

Interactive and headless modes use the same ordering contract for each 16 ms
logic step:

1. Drain a bounded number of adapter commands.
2. Apply commands and produce acknowledgements or protocol errors.
3. Apply locally generated DAS/ARR actions when interactive.
4. Tick `GameState` once with `TICK_MS`.
5. Capture critical events and publish observations when due.
6. Render from `GameSnapshot` when interactive.

Changing this order can change deterministic AI trajectories and therefore
requires tests plus updates to `rules-spec.md` or `adapter.md`.

## Performance Contracts

- `GameState::tick`, input updates, snapshot projection, and framebuffer rendering
  are covered by allocation-gate tests.
- Board cells are copied only when `board_id` changes; metadata is refreshed into
  a reusable snapshot.
- Adapter observations use `Arc` fanout and are not built without streaming
  subscribers.
- Terminal output uses framebuffer diffs and explicit invalidation after resize.

Run `cargo test` for correctness and allocation gates. Run `cargo bench` followed
by `python3 scripts/bench_gate.py` for performance regression checks.

## Structural Improvement Plan

The largest files are orchestration and acceptance-test hotspots, not a reason for
a broad rewrite. Decompose them incrementally when the associated behavior is
changed:

1. ✅ Extract the duplicated adapter command/observation pump from interactive and
   headless loops behind a small stateful internal type.
2. Separate adapter server connection handling, controller policy, and buffered
   writer/fanout logic while preserving the public protocol surface.
3. Move `GameState` unit tests into behavior-focused test modules so rule changes
   do not keep growing the production source file.
4. Factor shared TCP test-client helpers out of adapter acceptance/e2e suites.

Each extraction must start with characterization tests and keep the allocation
and protocol acceptance gates green.
