#!/usr/bin/env python3
"""
Criterion benchmark gate for tui-tetris.

Usage:
  python3 scripts/bench_gate.py
  python3 scripts/bench_gate.py --run

This script reads `target/criterion/*/new/estimates.json` and enforces
simple upper-bound thresholds on the median point estimate (in seconds).

Keep thresholds intentionally generous to avoid flaky failures across machines,
but tight enough to catch obvious regressions.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


THRESHOLDS_SECONDS: dict[str, float] = {
    # "nano" benchmarks: keep very generous (these are extremely machine-dependent).
    "game_tick_16ms": 20e-9,  # 20ns
    "clear_4_lines": 200e-9,  # 200ns
    "snapshot_meta_into": 200e-9,  # 200ns
    "snapshot_board_into": 200e-9,  # 200ns
    # "micro" benchmarks: these are the ones most likely to regress materially.
    "build_observation+to_writer": 5e-6,  # 5us
    "build_observation_only": 3e-6,  # 3us
    "serialize_observation_to_writer": 5e-6,  # 5us
    "serialize_observation_to_writer_dynamic": 8e-6,  # 8us
    "render_into": 10e-6,  # 10us
    "encode_diff_into": 20e-6,  # 20us
    "encode_diff_into_noop": 5e-6,  # 5us
    # JSON parsing can vary a lot by CPU and serde_json version; keep generous.
    "parse_command_action": 50e-6,  # 50us
}


def read_median_seconds(estimates_path: Path) -> float:
    data = json.loads(estimates_path.read_text(encoding="utf-8"))
    median = data.get("median")
    if not isinstance(median, dict):
        raise ValueError("missing median")
    point = median.get("point_estimate")
    if not isinstance(point, (int, float)):
        raise ValueError("missing median.point_estimate")
    # Criterion's `estimates.json` stores values in nanoseconds for time-based benches
    # (even though the CLI output may format them as ns/us/ms).
    return float(point) * 1e-9


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--run",
        action="store_true",
        help="Run `cargo bench` (bench profile) before checking thresholds.",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]

    if args.run:
        proc = subprocess.run(["cargo", "bench"], cwd=repo_root)
        if proc.returncode != 0:
            return proc.returncode

    criterion_root = repo_root / "target" / "criterion"
    if not criterion_root.exists():
        print("Missing target/criterion. Run `cargo bench` first.", file=sys.stderr)
        return 2

    failed: list[str] = []
    for name, limit in sorted(THRESHOLDS_SECONDS.items()):
        estimates = criterion_root / name / "new" / "estimates.json"
        if not estimates.exists():
            failed.append(f"{name}: missing {estimates}")
            continue
        try:
            median_s = read_median_seconds(estimates)
        except Exception as e:
            failed.append(f"{name}: failed to parse estimates.json ({e})")
            continue
        if median_s > limit:
            failed.append(
                f"{name}: median {median_s:.3e}s > limit {limit:.3e}s"
            )

    if failed:
        print("Benchmark gate failed:", file=sys.stderr)
        for line in failed:
            print(f"  - {line}", file=sys.stderr)
        return 1

    print("Benchmark gate OK.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
