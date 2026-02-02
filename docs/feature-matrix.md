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

## Terminal Rendering

- Custom framebuffer ✅
- Diff-based flush ✅
- Resizing invalidation ✅
- Snapshot-style renderer tests ✅

## Adapter

- TCP server (tokio) ✅
- JSON line framing ✅
- hello/welcome handshake ✅
- Controller/observer enforcement ✅
- Backpressure errors ✅
- Monotonic seq enforcement ✅
- Best-effort seq echo on parse errors ✅
- Wire logging (`TETRIS_AI_LOG_PATH`) ✅
- Immediate snapshot on hello ✅
- Closed-loop stability tests ✅

## Performance

- Core hot paths allocation-free (gate test) ✅
- End-to-end allocation-free (input + adapter + render) ⚠️
- Benchmarks (`cargo bench`) ❌
