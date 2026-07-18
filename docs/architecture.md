# Architecture

This document describes the current dependency boundaries and runtime flows.
Rule details belong in `rules-spec.md`; wire compatibility belongs in
`adapter.md`. The completed redesign is recorded in
`reimplementation-plan.md`.

## Dependency Boundaries

```text
app (`tui-tetris`)
  |-- tetris-adapter ---------> tetris-session -> tetris-core
  |                  `--------> tetris-adapter-protocol -> tetris-core
  |-- tetris-terminal --------> tetris-core
  `-- observe/replay CLI -----> immutable protocol/session APIs
```

- `core` owns deterministic game state, rules, scoring, timing, RNG, and raw
  snapshot projection. It has no terminal, network, serde, or async dependency.
- `engine::session` is the authoritative application transition boundary. It
  owns `GameState` and `SnapshotStore`, applies remote commands before local
  actions, ticks core once, returns `Transition`, and refreshes one coherent
  snapshot.
- `engine::fixed_step` owns pure elapsed-time/backlog accounting. It retains
  fractional and excess backlog while limiting one outer-loop burst to eight
  steps.
- `input` translates terminal key state into queued `GameAction` values. It never
  mutates game state.
- `term` renders immutable `GameViewModel` values through a framebuffer and
  diff flush. It never mutates game rules.
- `adapter` owns TCP framing, the client broker, per-client mailboxes,
  observation scheduling, and the sync/async bridge; wire types live in the
  separate adapter-protocol crate.
- `main` is only a composition root. Interactive and headless modes install
  different ports around the same `step_session` implementation.

Architecture boundary tests prevent platform imports in `core`, direct game
mutation from `main`, duplicate adapter outbound variants, and unbounded
production adapter channels. Cargo compiles core, session, protocol, adapter,
and terminal as independent source-owning workspace packages.

## Authoritative Fixed Step

Every interactive and headless 16 ms step calls the same path:

1. Drain at most 32 accepted remote commands.
2. Apply remote commands and record their outcomes.
3. Apply queued local press and DAS/ARR actions.
4. Tick `GameState` exactly once with `TICK_MS`.
5. Capture zero to four ordered core events into `Transition` and observation
   scheduling.
6. Refresh `SnapshotStore` (board only on `board_id` change, metadata always).
7. Deliver correlated replies and publish an observation when due.

Initial key presses are queued until this boundary; terminal event handling no
longer mutates `GameState` between ticks. `FixedStepClock` retains elapsed
backlog and returns at most eight steps per outer-loop iteration.

## Snapshot and Event Ownership

`SessionRuntime` is the only production owner of mutable game state.
`SnapshotStore` guarantees that consumers cannot combine current metadata with
an obsolete board. `Transition.changed` compares coherent before/after snapshots
and therefore includes timer and step-counter-only changes.

Core lock/line-clear events are captured once at the session boundary and
copied into `Transition`. Adapter scheduling consumes that result rather than
reaching into `GameState`, so additional replay or diagnostic consumers can use
the same transition.

## Adapter Concurrency and Backpressure

- `BrokerState` contains the client registry and the single authoritative
  `controller_id` under one lock. There is no per-client controller flag.
- Disconnect removal and lowest-id eligible promotion are one broker write
  transaction.
- Accepted commands carry a responder for their originating per-client mailbox.
  Ack, error, and targeted snapshot delivery bypass the global dispatcher;
  reliable overflow closes only that slow client.
- A streaming hello awaits bounded command-queue capacity for its required
  initial snapshot request. It cannot silently lose that request.
- There is no global reliable reply channel; correlated replies go directly to
  the originating client's bounded mailbox.
- Broadcast observations use one latest-only watch slot and one typed
  `Arc<ObservationMessage>` representation.
- Each client has a 32-message reliable queue and one replaceable observation.
- Adapter status is latest-only; wire logging has a bounded 1,024-record
  best-effort queue.
- Socket and disk I/O are never awaited while broker state is locked.
- Writer shutdown and authoritative startup bind remain bounded.

## State Hashing

Adapter `state_hash` uses canonical field-by-field FNV-1a encoding with explicit
integer byte order and enum codes. It does not depend on Rust's `Hash`
implementation or platform layout. Board bytes remain cached in the core
snapshot and are recomputed only when the board revision changes.

Protocol v3 observations include the authoritative logical step and all events.
Successful command ack messages include their correlation sequence, applied
step, and resulting state hash.

## Performance Contracts

- Core tick, unified session/input/observation/render no-I/O flow, adapter
  observation build/serialization, and terminal rendering have allocation gates.
- Framebuffer output remains diff-based and skips unchanged writes/flushes.
- Observation construction is skipped when no streaming subscriber exists.
- Criterion covers active tick, snapshot paths, observation serialization,
  command parsing, diff encoding, render pipelines, and injected writer dispatch.

Run `cargo test` for correctness and allocation gates. Run `cargo bench` followed
by `python3 scripts/bench_gate.py` for absolute performance regression checks.
