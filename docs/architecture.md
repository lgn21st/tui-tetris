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
  acknowledgements. `observation` owns snapshot-to-wire projection and stable
  state hashing; `client_mailbox` owns per-client reliable queues, latest-only
  observation slots, and slow-client cancellation; `wire_log` owns bounded
  best-effort diagnostic persistence. `server_config` owns environment parsing
  and listen-address construction, while `runtime` waits for the server's single
  authoritative bind result. Game commands cross the boundary as typed messages.
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

Both runners preserve fixed-step backlog across outer-loop iterations and process
at most eight catch-up steps per iteration. This bounds one burst of work without
discarding elapsed simulation time.

Changing this order can change deterministic AI trajectories and therefore
requires tests plus updates to `rules-spec.md` or `adapter.md`.

## Performance Contracts

- `GameState::tick`, input updates, snapshot projection, and framebuffer rendering
  are covered by allocation-gate tests.
- Board cells are copied only when `board_id` changes; metadata is refreshed into
  a reusable snapshot.
- Adapter observations use `Arc` fanout and are not built without streaming
  subscribers.
- Adapter input framing is incrementally bounded at 64 KiB per JSON line, so a
  client cannot force unbounded allocation by withholding a newline.
- Each client has a 32-message reliable output queue plus one replaceable
  observation slot. Reliable overflow disconnects only that client; stale
  observations are coalesced instead of accumulated.
- Adapter status is a latest-value channel, and wire logging uses a bounded 1,024
  record queue with best-effort drops, so connection churn and slow storage cannot
  create unbounded internal backlogs.
- Adapter startup reports the result of the actual async TCP bind; it does not
  probe and release the address before starting the server.
- Terminal output uses framebuffer diffs and explicit invalidation after resize.

Run `cargo test` for correctness and allocation gates. Run `cargo bench` followed
by `python3 scripts/bench_gate.py` for performance regression checks.
Renderer pipeline gates measure snapshot-to-framebuffer rendering, diff encoding,
and framebuffer swapping. Backend gates also exercise `TerminalRenderer` write and
flush dispatch through an injected writer; terminal device I/O remains outside the
reproducible gate.

Future structural work is tracked only in `roadmap.md`; this document describes
the architecture that exists today.
