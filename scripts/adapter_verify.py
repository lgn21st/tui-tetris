#!/usr/bin/env python3
"""Run the current conformance client maintained by this repository."""

from pathlib import Path
import runpy


CLIENT = (
    Path(__file__).resolve().parents[1]
    / "protocol"
    / "adapter"
    / "conformance"
    / "adapter_verify.py"
)

runpy.run_path(str(CLIENT), run_name="__main__")
