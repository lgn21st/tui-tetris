# Feature Matrix

Legend: ✅ implemented, ⚠️ partial, ❌ missing

## Core Rules

- 10x20 board ✅
- 7-bag RNG ✅
- SRS rotation + kicks ✅
- Hold ✅
- Lock delay + reset limit ✅
- Line clear pause ✅
- Scoring: line clears ✅
- Scoring: T-spins ✅
- Scoring: combo ✅
- Scoring: back-to-back ✅
- Determinism (same seed + same actions => same state_hash sequence) ✅

## Input

- Key mapping ✅
- DAS/ARR ✅
- Terminals without key release events (timeout auto-release) ✅
- Vim movement keys (`h/j/l`) supported ✅
- Terminals with repeat-but-no-release events (adaptive repeat-driven auto-release) ✅
- Initial presses and repeats share the fixed-step command boundary ✅

## Runtime Architecture

- One authoritative interactive/headless session step ✅
- Reusable fixed-step backlog clock with eight-step burst cap ✅
- Coherent single-owner snapshot cache ✅
- Explicit transition result with bounded ordered events ✅
- Machine-checked core/composition/queue boundaries ✅
- Stable command replay with per-step hashes and minimal failure prefixes ✅
- Source-owning core/session/protocol/adapter/terminal workspace packages ✅
- Replay TTR2 record/verify/inspect CLI ✅
- Finite deterministic headless and diagnostic CLI ✅

## Terminal Rendering

- Custom framebuffer ✅
- Diff-based flush ✅
- Resizing invalidation ✅
- Unchanged frames skip terminal write and flush ✅
- Injectable renderer output backend ✅
- Snapshot-style renderer tests ✅
- GameView allocation-free gate ✅
- Remote observer renderer mode (`cargo run -- observe ...`) ✅
- Immutable terminal `GameViewModel` and platform-neutral `InputCommand` ✅

## Adapter

- TCP server (tokio) ✅
- JSON line framing ✅
- Bounded inbound frames (64 KiB, incremental enforcement) ✅
- Bounded reliable outbound queues with slow-client isolation ✅
- Latest-only observation and status delivery under backpressure ✅
- hello/welcome handshake ✅
- Welcome advertises `features_always` / `features_optional` ✅
- Controller/observer enforcement ✅
- Backpressure errors ✅
- Monotonic seq enforcement ✅
- Best-effort seq echo on parse errors ✅
- Bounded best-effort wire logging (`TETRIS_AI_LOG_PATH`) ✅
- Authoritative bind startup/error propagation ✅
- Immediate snapshot on hello ✅
- Drift-free fixed-step observation cadence ✅
- Closed-loop stability tests ✅
- Required hello snapshot waits for bounded capacity ✅
- Direct per-client post-application replies ✅
- Single-variant latest-only global observation bridge ✅
- Single broker controller source of truth ✅
- Canonical field-by-field state hash encoding ✅
- Protocol v3 causal `logical_step`, `events[]`, and applied-state ack ✅
- Disconnect-storm, slow-client, and 32-observer stress gates ✅

## Performance

- Core hot paths allocation-free (gate test) ✅
- End-to-end allocation-free (input + adapter + render, no I/O) ✅
- Benchmarks (`cargo bench`) ✅
- Benchmark regression gate (`python3 scripts/bench_gate.py`) ✅
- Active-state tick benchmark (does not collapse into game-over early return) ✅
- Full renderer pipeline and injected-backend benchmarks ✅
- 100,000-step zero-allocation soak ✅
