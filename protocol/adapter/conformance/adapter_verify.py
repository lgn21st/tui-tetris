#!/usr/bin/env python3
"""Stdlib-only conformance client for Tetris AI Adapter Protocol 2.1.1."""

from __future__ import annotations

import argparse
import json
import socket
import sys
import time
from collections.abc import Callable
from typing import Any

PROTOCOL_VERSION = "2.1.1"


class AdapterClient:
    def __init__(self, host: str, port: int, timeout: float) -> None:
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.sock.settimeout(timeout)
        self.reader = self.sock.makefile("r", encoding="utf-8", newline="\n")
        self.timeout = timeout

    def close(self) -> None:
        self.reader.close()
        self.sock.close()

    def send(self, message: dict[str, Any]) -> None:
        payload = json.dumps(message, separators=(",", ":")) + "\n"
        self.sock.sendall(payload.encode("utf-8"))

    def read(self) -> dict[str, Any]:
        line = self.reader.readline()
        if not line:
            raise RuntimeError("adapter closed the connection")
        message = json.loads(line)
        if not isinstance(message, dict):
            raise RuntimeError("adapter message is not a JSON object")
        return message

    def wait_for(self, predicate: Callable[[dict[str, Any]], bool]) -> dict[str, Any]:
        deadline = time.monotonic() + self.timeout
        while time.monotonic() < deadline:
            message = self.read()
            if message.get("type") == "error":
                raise RuntimeError(f"adapter error: {message}")
            if predicate(message):
                return message
        raise TimeoutError("timed out waiting for adapter message")

    def hello(self, *, role: str, stream: bool) -> tuple[dict[str, Any], dict[str, Any] | None]:
        self.send(
            {
                "type": "hello",
                "seq": 1,
                "ts": now_ms(),
                "client": {"name": "adapter-verify", "version": "1.0.0"},
                "protocol_version": PROTOCOL_VERSION,
                "formats": ["json"],
                "requested": {
                    "stream_observations": stream,
                    "command_mode": "action",
                    "role": role,
                },
            }
        )
        welcome = self.wait_for(lambda message: message.get("type") == "welcome")
        observation = None
        if stream:
            observation = self.wait_for(lambda message: message.get("type") == "observation")
        return welcome, observation


def now_ms() -> int:
    return int(time.time() * 1000)


def command(seq: int, actions: list[str], seed: int | None = None) -> dict[str, Any]:
    message: dict[str, Any] = {
        "type": "command",
        "seq": seq,
        "ts": now_ms(),
        "mode": "action",
        "actions": actions,
    }
    if seed is not None:
        message["restart"] = {"seed": seed}
    return message


def validate_observation(message: dict[str, Any]) -> None:
    required = {
        "type", "seq", "ts", "playable", "paused", "game_over", "episode_id",
        "seed", "piece_id", "step_in_piece", "board", "board_id", "next",
        "next_queue", "can_hold", "state_hash", "score", "level", "lines", "timers",
    }
    missing = sorted(required.difference(message))
    if missing:
        raise RuntimeError(f"observation missing fields: {missing}")
    board = message["board"]
    if not isinstance(board, dict):
        raise RuntimeError("observation board is not an object")
    cells = board.get("cells")
    if board.get("width") != 10 or board.get("height") != 20:
        raise RuntimeError("observation board dimensions are not 10x20")
    if not isinstance(cells, list) or len(cells) != 20 or any(len(row) != 10 for row in cells):
        raise RuntimeError("observation cells are not a 20x10 matrix")
    queue = message["next_queue"]
    if not isinstance(queue, list) or len(queue) != 5 or message["next"] != queue[0]:
        raise RuntimeError("next/next_queue invariant failed")


def verify_ready(args: argparse.Namespace) -> None:
    client = AdapterClient(args.host, args.port, args.timeout)
    try:
        welcome, observation = client.hello(role="observer", stream=True)
        if welcome.get("role") != "observer":
            raise RuntimeError(f"observer role was not preserved: {welcome}")
        assert observation is not None
        validate_observation(observation)
    finally:
        client.close()


def verify_claim(args: argparse.Namespace) -> None:
    client = AdapterClient(args.host, args.port, args.timeout)
    try:
        welcome, _ = client.hello(role="controller", stream=False)
        if welcome.get("role") != "controller":
            raise RuntimeError(f"controller was not assigned: {welcome}")
        client.send({"type": "control", "seq": 2, "ts": now_ms(), "action": "claim"})
        client.wait_for(lambda message: message.get("type") == "ack" and message.get("seq") == 2)
    finally:
        client.close()


def restart_episode(client: AdapterClient, baseline: dict[str, Any], seed: int) -> dict[str, Any]:
    client.send(command(2, ["restart"], seed))
    ack_seen = False
    restarted = None
    deadline = time.monotonic() + client.timeout
    while time.monotonic() < deadline and (not ack_seen or restarted is None):
        message = client.read()
        if message.get("type") == "error":
            raise RuntimeError(f"restart failed: {message}")
        if message.get("type") == "ack" and message.get("seq") == 2:
            ack_seen = True
        if (
            message.get("type") == "observation"
            and message.get("episode_id") != baseline.get("episode_id")
            and message.get("seed") == seed
            and message.get("playable") is True
        ):
            restarted = message
    if not ack_seen or restarted is None:
        raise TimeoutError("restart did not produce both ack and fresh observation")
    return restarted


def collect_signature(args: argparse.Namespace, seed: int, pieces: int) -> list[tuple[str, ...]]:
    client = AdapterClient(args.host, args.port, args.timeout)
    try:
        _, baseline = client.hello(role="controller", stream=True)
        assert baseline is not None
        current = restart_episode(client, baseline, seed)
        signature = [tuple(current["next_queue"])]
        seq = 2
        for _ in range(pieces - 1):
            seq += 1
            previous_piece = current["piece_id"]
            client.send(command(seq, ["hardDrop"]))
            ack_seen = False
            next_observation = None
            deadline = time.monotonic() + client.timeout
            while time.monotonic() < deadline and (not ack_seen or next_observation is None):
                message = client.read()
                if message.get("type") == "error":
                    raise RuntimeError(f"hardDrop failed: {message}")
                if message.get("type") == "ack" and message.get("seq") == seq:
                    ack_seen = True
                if (
                    message.get("type") == "observation"
                    and message.get("episode_id") == current["episode_id"]
                    and message.get("piece_id") != previous_piece
                ):
                    next_observation = message
            if not ack_seen or next_observation is None:
                raise TimeoutError("hardDrop did not produce ack and next-piece observation")
            current = next_observation
            signature.append(tuple(current["next_queue"]))
        return signature
    finally:
        client.close()


def verify_restart(args: argparse.Namespace) -> None:
    client = AdapterClient(args.host, args.port, args.timeout)
    try:
        _, baseline = client.hello(role="controller", stream=True)
        assert baseline is not None
        validate_observation(restart_episode(client, baseline, args.seed))
    finally:
        client.close()


def verify_determinism(args: argparse.Namespace) -> None:
    first = collect_signature(args, args.seed, args.pieces)
    time.sleep(0.05)
    second = collect_signature(args, args.seed, args.pieces)
    if first != second:
        raise RuntimeError(f"restart.seed mismatch: first={first}, second={second}")


CHECKS = {
    "ready": verify_ready,
    "claim": verify_claim,
    "restart": verify_restart,
    "determinism": verify_determinism,
}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("check", choices=[*CHECKS, "all"], nargs="?", default="all")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=7777)
    parser.add_argument("--timeout", type=float, default=2.0)
    parser.add_argument("--seed", type=int, default=123)
    parser.add_argument("--pieces", type=int, default=8)
    args = parser.parse_args()

    try:
        selected = CHECKS.items() if args.check == "all" else [(args.check, CHECKS[args.check])]
        for name, check in selected:
            check(args)
            print(f"OK: {name}")
            time.sleep(0.05)
    except (AssertionError, OSError, RuntimeError, TimeoutError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
