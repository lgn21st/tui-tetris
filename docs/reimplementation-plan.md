# Reimplementation Plan

This execution record rebuilds tui-tetris around one deterministic application
runtime. Product properties are preserved; replaceable historical API and wire
shapes are not. The redesign intentionally introduced protocol 3.0.0, Replay
TTR2, a new transition API, and real workspace ownership boundaries.

## Outcomes

- One authoritative 16 ms step implementation for interactive, headless, tests,
  replay, and future training runners.
- All local and remote commands are applied at a fixed-step boundary in a stable
  order: remote commands, local commands, core tick, events, observations.
- Domain events are returned by a step and are not destructively consumed from
  `GameState` by a particular adapter.
- One snapshot cache owns board-copy invalidation and metadata refresh.
- Every cross-thread queue is bounded or explicitly latest-only.
- Adapter controller/client state has one owner and one controller source of
  truth; socket writers remain isolated per client.
- Core, session, protocol, adapter, and terminal boundaries are
  compiler-enforced as separately owned crates.

## Preserved Product Properties

The reimplementation preserves:

- every rule and timing constant in `docs/rules-spec.md`;
- equal-seed/equal-command deterministic trajectories;
- the 16 ms tick, retained backlog, and eight-step catch-up cap;
- external control, typed errors, deterministic ordering, roles, and the TCP
  profile; protocol v2 shape compatibility was deliberately removed;
- post-application acknowledgements and atomic place failure;
- framebuffer diff rendering and current input mappings/DAS/ARR behavior;
- all existing correctness, conformance, allocation, and benchmark gates.

## Target Architecture

```text
terminal events ----> local command queue ---+
                                             |
adapter commands ---> remote command queue --+--> SessionRuntime::transition
                                                    |
                                                    +--> Transition/events
                                                    +--> SnapshotStore
                                                              |
                                            +-----------------+-----------------+
                                            |                                   |
                                      terminal renderer                 observation scheduler
                                                                                |
                                                                         adapter broker
```

### Domain layer

`core` remains deterministic and dependency-free. `GameState` continues to be
the aggregate root during migration, but its transition result becomes explicit.
Longer term it is decomposed internally into board/active-piece, randomizer,
progress/scoring, timers, and lifecycle state without exposing mutation.

### Application layer

`SessionRuntime` owns `GameState` and `SnapshotStore`. Its `transition` method is
the only application-level path that advances gameplay. It accepts one bounded
`StepInput` and returns a fixed-capacity `Transition` event/outcome collection.

`SnapshotStore` owns the cached board revision and always refreshes metadata. A
caller cannot accidentally publish current metadata with an obsolete board.

### Ports and adapters

- Crossterm translates raw key events into local commands but never mutates the
  game directly.
- The TCP adapter translates wire messages into remote commands and receives
  typed acknowledgements/observations.
- Rendering consumes immutable snapshots only.
- Observe mode is a protocol client and never becomes authoritative state.

## Delivery Phases

### Phase 0: Characterization and design

- Keep the existing full suite green as the compatibility baseline.
- Add this plan and an architecture acceptance checklist.
- Add deterministic runtime tests before introducing production runtime code.

Exit: behavior and non-functional invariants are explicitly testable.

### Phase 1: Unified deterministic session

- Introduce `SessionRuntime`, `SnapshotStore`, `LocalCommands`, and `Transition`.
- Apply remote commands before local actions, then tick exactly once.
- Capture lock/line-clear events exactly once and expose them in `Transition`.
- Centralize observation cadence and cached snapshot refresh.
- Add differential tests against direct `GameState` transitions.
- Add an allocation gate for a warmed session step.

Exit: headless behavior can be expressed entirely through `SessionRuntime`.

### Phase 2: Runner replacement

- Replace duplicated interactive/headless step bodies with the same runtime.
- Queue initial key presses, rotations, pause, restart, and hard drop until the
  next step boundary; retain press responsiveness within one 16 ms step.
- Keep terminal polling, resize handling, and rendering outside the domain.
- Move fixed-step backlog calculation into a tested reusable clock component.

Exit: one production function defines command/tick/observation ordering.

### Phase 3: Bounded adapter bridge

- Replace the global unbounded outbound channel with bounded reliable delivery
  plus a latest-only broadcast observation slot.
- Give initial snapshot requests a non-dropping/latest-only request path instead
  of competing silently with gameplay command capacity.
- Surface closed/full bridge outcomes explicitly; never ignore a required reply.

Exit: all adapter queues have documented finite bounds or coalescing semantics.

### Phase 4: Single-owner adapter broker

- Move client registry, controller policy, sequence state, and promotion into one
  broker state/state machine protected by one short-lived lock.
- Remove per-client duplicated `is_controller`; derive authorization from the
  broker's single `controller_id`.
- Keep framing readers and bounded writers as per-connection tasks.
- Unit-test broker transitions without TCP, leaving thin transport e2e tests.

Exit: no multi-lock controller/client protocol remains in socket handlers.

### Phase 5: Boundary hardening

- Reduce `GameState` public surface to commands, steps, and snapshots.
- Use canonical field-by-field state hashing independent of Rust's `Hash`
  implementation details.
- Consolidate duplicated protocol outbound variants around typed reliable and
  latest observation messages.
- Replace acceptance-test engine replicas with the production session harness.
- Split `core` and protocol into workspace crates only after dependency and API
  tests prove the boundary; avoid a layout-only migration.

Exit: compiler-enforced core/protocol/application dependency direction.

### Phase 6: Validation and removal

- Remove superseded loops, state flags, channels, helpers, and test harnesses.
- Update architecture, rules notes, feature matrix, roadmap, and changelog.
- Run full tests, adapter conformance/e2e/closed-loop tests, allocation gates,
  Clippy, formatting, diff checks, complete benchmarks, and absolute gates.

Exit: no compatibility shim or duplicate authoritative path remains.

## Test Strategy

- Core: table and state-machine tests for rules, edge cases, overflow, and seed
  determinism.
- Session: command ordering, one-tick semantics, events, snapshot coherence,
  catch-up behavior, and old/new differential traces.
- Adapter broker: pure lifecycle/sequence/backpressure transition tests.
- TCP: framing, flush/ack order, bind errors, disconnect cleanup, stalled client
  isolation, and black-box conformance.
- Rendering/input: snapshot output and deterministic logical-time repeat tests.
- Performance: warmed allocation gates plus representative active-state
  Criterion benchmarks; device and socket I/O stay outside deterministic gates.

## Rollout and Rollback

Each phase was locked by a failing test before implementation and validated at
the narrowest owning crate before integration. Wire and replay changes use
explicit major/container versions rather than compatibility shims. No
persistent data migration is involved.

## Definition of Done

- Interactive and headless execution call the same session step implementation.
- Local input never mutates `GameState` outside a fixed-step boundary.
- Domain events support multiple consumers without destructive competition.
- Snapshot coherence is owned by one component.
- No unbounded adapter channel exists.
- Controller state has one owner/source of truth.
- The production session has direct TCP acceptance coverage; test-only protocol
  fixtures cannot define or replace production runtime behavior.
- Protocol 3.0.0 conformance, deterministic trajectories, zero-allocation gates,
  and benchmark thresholds all pass.

## Execution Status (2026-07-18)

- Phases 0-4 are implemented: the unified session/clock is in production,
  interactive and headless runners share one step, all local gameplay input is
  queued, snapshots/events have one application owner, outbound delivery is
  bounded/latest-only, required initial snapshots cannot be silently dropped,
  and broker controller state has one source of truth.
- Phase 5 is complete: core, session, adapter protocol, adapter, and terminal
  compile as separately owned workspace packages with no cross-tree source
  indirection; canonical hashing is shared by replay and protocol;
  acceptance/closed-loop drivers use the production session; correlated replies
  are direct; and the global bridge has one latest-only Arc observation type.
- Replay TTR2 records versioned rulesets and complete transition hashes, reports
  the first mismatch, produces a minimal prefix, and exposes
  `record/verify/inspect` CLI commands.
- Stress coverage includes disconnect storms, reliable-output flooding with a
  healthy peer, queue saturation, reconnect loops, and 32-observer fanout.
- Protocol v3 exposes logical steps and event collections in observations and
  binds successful command acknowledgements to applied step and state hash.
- Phase 6 is complete for this change set: the full test suite, protocol helper
  entry points, Clippy with warnings denied, formatting, diff checks, all
  allocation gates, the 200-episode closed loop, Criterion benchmarks, and the
  absolute benchmark gate pass.
