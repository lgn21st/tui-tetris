# Development Workflow

Every behavioral change moves through the dependency graph in this order. A
later layer may project or transport behavior, but it may not redefine it.

1. **Rules** — change the written rule/timing contract and add a failing
   characterization test.
2. **Core** — implement deterministic state transitions in `tetris-core`; no
   terminal, network, async, serialization, or wall-clock dependencies.
3. **Session and replay** — expose behavior through `StepInput → Transition`,
   record a TTR2 `StepRecord`, and verify an equal-seed replay trajectory.
4. **Adapter** — map authenticated protocol commands to the session and project
   immutable snapshots; correlated replies use the originating responder.
5. **Terminal** — map input into queued commands and render snapshots only.

Required local gate before handoff:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Adapter lifecycle changes additionally run acceptance, e2e, stress, closed-loop,
and the ignored 200-episode test. Hot-path changes additionally run every
allocation gate, the complete Criterion suite, and `scripts/bench_gate.py`.

When a replay mismatch occurs, save `ReplayTape::minimal_failure_prefix` rather
than a full long trajectory; it contains the first divergent step and is the
smallest prefix needed to reproduce the failure.
