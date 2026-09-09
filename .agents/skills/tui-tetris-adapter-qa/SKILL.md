---
name: tui-tetris-adapter-qa
description: Review, test, and evolve tui-tetris AI adapter design and implementation against its current protocol package and local implementation profile. Use for protocol messages, controller/observer lifecycle, sequencing, acknowledgements, observations, restart determinism, TCP framing, startup errors, concurrency, queue limits, slow-client isolation, wire logging, conformance tooling, or adapter documentation in /Users/daniel/workspace/learn/tui-tetris.
---

# TUI Tetris Adapter QA

Work from `/Users/daniel/workspace/learn/tui-tetris` and use strict TDD.

## Read the Relevant Contract

Always read `AGENTS.md`, the adapter index at `docs/adapter.md`, and the current version in `protocol/adapter/VERSION`. Read `protocol/adapter/SPEC.md`, the selected transport profile, `schema.json`, and `CHANGELOG.md`. Read `docs/adapter-tui-tetris.md` for local implementation decisions. Then read only the implementation areas in scope:

- Wire types and parsing: `crates/tetris-adapter-protocol/src/protocol.rs`
- TCP connection/control flow: `crates/tetris-adapter/src/adapter/server.rs`
- Startup and sync/async bridge: `crates/tetris-adapter/src/adapter/runtime.rs`, `crates/tetris-adapter/src/adapter/server_config.rs`
- Per-client delivery/backpressure: `crates/tetris-adapter/src/adapter/client_mailbox.rs`
- Diagnostic persistence: `crates/tetris-adapter/src/adapter/wire_log.rs`
- Command application: `crates/tetris-adapter/src/adapter/game_loop.rs`, `crates/tetris-adapter/src/adapter/command_apply.rs`
- Snapshots and cadence: `crates/tetris-adapter/src/adapter/observation.rs`, `crates/tetris-adapter/src/adapter/observation_schedule.rs`

Treat the current `SPEC.md`, schema, and selected transport profile as the portable compatibility contract. Treat `docs/adapter-tui-tetris.md` as this repository's implementation profile. Never require another project to copy local queue capacities, runtime primitives, logging internals, or scheduling algorithms.

## Choose the TDD Boundary

- Protocol parsing, validation, or error mapping: unit tests in `protocol.rs` or `server_tests.rs`.
- Role, sequencing, command, and documented acceptance behavior: `tests/adapter_acceptance_test.rs`.
- TCP framing, flush/ack ordering, bind errors, disconnects, and status: `tests/adapter_e2e_test.rs`.
- Reconnect, restart seeds, deterministic trajectories, or hangs: `tests/adapter_closed_loop_test.rs`.
- Documentation examples/schema/constants: `tests/adapter_docs_test.rs`.
- Mailbox or logger queue policy: colocated unit tests in `client_mailbox.rs` or `wire_log.rs`.
- Slow-client isolation: pair mailbox unit coverage with a TCP-level test proving a stalled client is closed within a bound while another client remains responsive. Prefer a deterministic capacity or test hook over a large, timing-sensitive socket flood.

Add the failing test before implementation and confirm the failure represents the intended contract.

## Verify Protocol Semantics

1. Require `hello` before command/control and enforce compatible semver plus `seq=1`.
2. Enforce strictly increasing client sequences; echo triggering sequences in welcome/ack/error.
3. Keep at most one controller; preserve observer locks, claim/release rules, and lowest-id eligible promotion on controller disconnect.
4. Acknowledge gameplay commands only after the fixed-step game loop applies them.
5. Reject a full inbound command queue with `backpressure` without enqueueing or applying the command.
6. Keep place application atomic on every error path.
7. Preserve deterministic restart seed, episode, queue, and state-hash behavior.

## Verify Concurrency and Resource Bounds

- Accept at most 65,536 payload bytes per inbound JSON line and reject invalid UTF-8 without unbounded buffering.
- Keep the inbound command channel bounded.
- Keep reliable per-client output bounded; never silently drop welcome/ack/error. On reliable overflow, isolate and close only the slow client.
- Keep at most one pending observation per client; newer full snapshots may replace older pending snapshots, so sequence gaps are valid.
- Keep adapter status latest-only rather than accumulating connection history.
- Keep wire logging bounded and best-effort; disk latency or logger failure must not block sockets, protocol ordering, or game state.
- Await no socket or disk I/O while holding shared controller/client locks.
- When both locks are required, acquire controller before clients and release them before slow work.
- Bound writer shutdown so dead peers cannot retain tasks indefinitely.
- Use one authoritative async bind and propagate its success/error; do not probe and release the address before server startup.

## Verify Observation Semantics

- Emit full snapshots with every required field and the documented board encoding.
- Keep `board_id`, `state_hash`, timers, `events[]`, hold, and queue semantics aligned with core snapshots.
- Preserve fixed-step phase-accumulator cadence and immediate critical-transition snapshots.
- Do not build periodic observations when no streaming subscriber exists.
- Preserve `Arc` fanout and the observation no-allocation gate.

## Synchronize Documentation

Update `protocol/adapter/` only for portable fields, errors, lifecycle semantics, or transport guarantees. Maintain one current protocol package: update its files in place, bump `VERSION` according to semantic versioning, and append upgrade guidance to `CHANGELOG.md`; never create per-version directories. After an upgrade, prepare a concise notification prompt for dependent project agents. Update `docs/adapter-tui-tetris.md` for queue policy, runtime configuration, scheduling, logging, startup, or other local choices. Keep `docs/adapter.md` as the navigation boundary between them. Update `docs/architecture.md`, `docs/feature-matrix.md`, and the project `CHANGELOG.md` when their claims are affected. Add or adjust `adapter_docs_test.rs` assertions for package completeness and boundary leaks.

## Validate

```bash
cargo test -p tetris-adapter --lib
cargo test --test adapter_acceptance_test
cargo test --test adapter_e2e_test
cargo test --test adapter_closed_loop_test
cargo test --test adapter_docs_test
cargo test --test adapter_observation_no_alloc_gate_test
cargo clippy --all-targets --all-features -- -D warnings
```

Also confirm the current conformance entry points load:

```bash
python3 scripts/adapter_verify.py --help
python3 protocol/adapter/conformance/adapter_verify.py --help
```

For lifecycle, reconnect, queue, or task-cleanup changes, also run:

```bash
cargo test --test adapter_closed_loop_test closed_loop_long_run_200_episodes -- --ignored --exact
```

If the environment forbids localhost binds, rerun TCP suites with loopback permission. Never skip them and claim adapter completion.
