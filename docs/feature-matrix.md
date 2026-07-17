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

## Terminal Rendering

- Custom framebuffer ✅
- Diff-based flush ✅
- Resizing invalidation ✅
- Unchanged frames skip terminal write and flush ✅
- Injectable renderer output backend ✅
- Snapshot-style renderer tests ✅
- GameView allocation-free gate ✅
- Remote observer renderer mode (`cargo run -- observe ...`) ✅

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

## Performance

- Core hot paths allocation-free (gate test) ✅
- End-to-end allocation-free (input + adapter + render, no I/O) ✅
- Benchmarks (`cargo bench`) ✅
- Benchmark regression gate (`python3 scripts/bench_gate.py`) ✅
- Active-state tick benchmark (does not collapse into game-over early return) ✅
- Full renderer pipeline and injected-backend benchmarks ✅
