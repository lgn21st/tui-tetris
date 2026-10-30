# Roadmap

The renderer is where the work has been (`docs/kitty-graphics.md`). What is left:

- Keep the allocation and Criterion gates green; `cargo bench` +
  `python3 scripts/bench_gate.py` cover the playfield layers too
- The side panel needs 12 columns beside the frame (49 columns for the default
  22-column frame). Below that it is skipped with a one-line stderr hint; a
  compact fallback would make narrow terminals usable
- Optional polish: a per-mino sprite atlas (~90 B per move instead of 0.4 KB) if
  the motion layer's bandwidth ever matters again, `U=1` Unicode placeholders if
  the well ever paints into a scrolling region, and protocol animation frames
  (`a=f`) for landing effects

Deliberately not done:

- The sprite atlas is not needed for smooth motion: the layered image already
  places the piece at any pixel offset, so the fall is interpolated between rows
  without it
- Cell-based rendering stays removed; there is no fallback path to maintain

Validate:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo bench && python3 scripts/bench_gate.py
python3 scripts/kitty_smoke_test.py --expect-square-minos
```
