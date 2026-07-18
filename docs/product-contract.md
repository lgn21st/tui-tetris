# Product Contract

The project is defined by outcomes, not by its current implementation.

## Non-negotiable properties

- **Deterministic** — equal ruleset, seed, and step inputs produce equal transitions.
- **Replayable** — every authoritative run can be recorded and independently verified.
- **Resource-bounded** — queues, events, frames, logs, and catch-up work have explicit bounds.
- **Externally controllable** — an AI can command the same authoritative simulation as a human.
- **Causally observable** — replies and events identify the logical step and resulting state.

Correctness includes explicit overflow, invalid-input, disconnect, and version-mismatch behavior.

## Replaceable policies

The following are design choices, not product invariants:

- a 16 ms real-time scheduler interval;
- gameplay timing, scoring tables, rotation rules, and board dimensions;
- JSON field layout, transport, protocol version, and compatibility policy;
- observation cadence and whether snapshots and events share a message;
- crate names, public Rust APIs, task layout, and rendering strategy.

A policy may change when tests, measurements, or a simpler model justify it. The
same change must update the ruleset/protocol version, replay metadata, tests, and
documentation. Compatibility shims require a named consumer and removal date.

## Decision order

1. Product properties above.
2. Evidence from deterministic tests, replay, stress, and measurement.
3. Simpler ownership and dependency direction.
4. Existing documentation and implementation.
