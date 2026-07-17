#!/usr/bin/env python3
"""Run the conformance client pinned by this repository."""

from pathlib import Path
import runpy


CLIENT = (
    Path(__file__).resolve().parents[1]
    / "protocol"
    / "adapter"
    / "v2.1.1"
    / "conformance"
    / "adapter_verify.py"
)

runpy.run_path(str(CLIENT), run_name="__main__")
