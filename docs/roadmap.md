# Roadmap

Open work:

- Keep allocation and Criterion gates green
- Shrink `GameState` public surface
- Split connection framing/writer when those flows next change

Validate:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Optional: `cargo bench` then `python3 scripts/bench_gate.py`; ignored 200-episode closed-loop test.
