# Roadmap

Open work:

- Keep allocation and Criterion gates green
- Rendering follow-ups (`docs/kitty-graphics.md`): per-mino sprite placements to
  cut the motion layer to ~90 bytes per move and enable sub-cell smooth motion,
  `U=1` Unicode placeholders for tmux scroll regions, and animation frames
  (`a=f`) for clear/landing effects

Validate:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Optional: `cargo bench` then `python3 scripts/bench_gate.py`; ignored 200-episode closed-loop test.
