#!/usr/bin/env python3
"""Smoke-test the Kitty graphics path without a real terminal.

Runs the game on a pseudo-terminal of a known size (with pixel dimensions, so
cell-pixel detection has something to work with), lets it draw a few frames,
then quits and inspects the captured byte stream.

Checks:
  * the alternate screen is entered and left,
  * escape sequences are only emitted when graphics are forced on,
  * every graphics payload is a well-formed, chunked APC,
  * `q=2` appears on every chunk so no `Gi=...;OK` leaks into the tty,
  * inside tmux every APC sits in a `DCS tmux;` passthrough envelope.

Usage: python3 scripts/kitty_smoke_test.py [--binary target/debug/tui-tetris]
                                           [--graphics auto|kitty|off] [--tmux]
                                           [--frames-ms 800]

`--graphics auto` (the default) leaves `TUI_TETRIS_GRAPHICS` unset, which is the
shipping behaviour: graphics are on unless the terminal is known to be dumb.
"""

from __future__ import annotations

import argparse
import base64
import errno
import zlib
import fcntl
import os
import select
import signal
import struct
import sys
import termios
import time

ESC = 0x1B

WELL_BG = (28, 28, 28)
PIECE_COLORS = {
    (79, 209, 196): "I",
    (245, 224, 94): "O",
    (158, 122, 235): "T",
    (105, 212, 145): "S",
    (252, 130, 130): "Z",
    (99, 178, 237): "J",
    (245, 173, 84): "L",
}


def control_values(control: str) -> dict[str, str]:
    return dict(
        (key, value) for key, value in (part.split("=", 1) for part in control.split(",") if "=" in part)
    )


def check_square_minos(control: str, pixels: bytes, channels: int) -> list[str]:
    """Cells must tile the grid exactly, and a piece must be one merged shape.

    Fully opaque pixels on the motion layer are the solid piece (shadow, ghost
    and flashes are translucent). Cells fill their whole band, so a piece is one
    blob whose width and height are exact multiples of the band size; a stretched
    or gapped grid shows up as a remainder.
    """
    values = control_values(control)
    width = int(values["s"])
    height = int(values["v"])
    if len(pixels) != width * height * channels:
        return [f"decoded {len(pixels)} bytes for {width}x{height}x{channels}"]
    if channels != 4:
        return []
    # The image covers the 10x20 mino grid; `c`/`r` are its text-cell box.
    band_w, band_h = width // 10, height // 20
    if band_w < 4 or band_h < 4:
        return [f"band {band_w}x{band_h}px is too small"]

    def solid(x: int, y: int) -> bool:
        return pixels[(y * width + x) * 4 + 3] == 255

    seen = bytearray(width * height)
    problems = []
    counted = 0
    for start_index in range(width * height):
        if seen[start_index] or not solid(start_index % width, start_index // width):
            continue
        stack = [start_index]
        seen[start_index] = 1
        min_x = max_x = start_index % width
        min_y = max_y = start_index // width
        area = 0
        while stack:
            index = stack.pop()
            area += 1
            x, y = index % width, index // width
            min_x, max_x = min(min_x, x), max(max_x, x)
            min_y, max_y = min(min_y, y), max(max_y, y)
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, ny = x + dx, y + dy
                if not (0 <= nx < width and 0 <= ny < height):
                    continue
                neighbour = ny * width + nx
                if seen[neighbour] or not solid(nx, ny):
                    continue
                seen[neighbour] = 1
                stack.append(neighbour)
        block_w = max_x - min_x + 1
        block_h = max_y - min_y + 1
        if block_w < band_w or block_h < band_h or area < band_w * band_h:
            continue
        counted += 1
        if block_w % band_w:
            problems.append(f"piece is {block_w}px wide, not a multiple of {band_w}px")
        if block_h % band_h:
            problems.append(f"piece is {block_h}px tall, not a multiple of {band_h}px")
    if counted == 0:
        problems.append("found no piece to measure")
    return problems


def check_preview_minos(
    control: str, pixels: bytes, side: int, require_pieces: bool
) -> list[str]:
    """HUD previews must be built from exact `side x side` mino squares."""
    values = control_values(control)
    width = int(values["s"])
    height = int(values["v"])
    if len(pixels) != width * height * 4:
        return [f"preview is {len(pixels)} bytes, not {width}x{height}x4"]

    def solid(x: int, y: int) -> bool:
        return pixels[(y * width + x) * 4 + 3] == 255

    seen = bytearray(width * height)
    problems = []
    counted = 0
    for start_index in range(width * height):
        if seen[start_index] or not solid(start_index % width, start_index // width):
            continue
        stack = [start_index]
        seen[start_index] = 1
        min_x = max_x = start_index % width
        min_y = max_y = start_index // width
        while stack:
            index = stack.pop()
            x, y = index % width, index // width
            min_x, max_x = min(min_x, x), max(max_x, x)
            min_y, max_y = min(min_y, y), max(max_y, y)
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, ny = x + dx, y + dy
                if not (0 <= nx < width and 0 <= ny < height):
                    continue
                neighbour = ny * width + nx
                if seen[neighbour] or not solid(nx, ny):
                    continue
                seen[neighbour] = 1
                stack.append(neighbour)
        block_w = max_x - min_x + 1
        block_h = max_y - min_y + 1
        if block_w < side or block_h < side:
            continue
        counted += 1
        if block_w % side:
            problems.append(f"preview piece is {block_w}px wide, not a multiple of {side}")
        if block_h % side:
            problems.append(f"preview piece is {block_h}px tall, not a multiple of {side}")
    if counted == 0 and require_pieces:
        problems.append("found no preview piece to measure")
    return problems


def inspect_board(control: str, pixels: bytes) -> list[str]:
    """The board layer is opaque RGB: background plus settled minos."""
    problems = []
    values = control_values(control)
    width = int(values["s"])
    height = int(values["v"])
    if len(pixels) != width * height * 3:
        problems.append(f"board layer is {len(pixels)} bytes, not {width}x{height}x3")
        return problems
    seen = {tuple(pixels[index : index + 3]) for index in range(0, len(pixels), 3)}
    if WELL_BG not in seen:
        problems.append("board background colour missing")
    cols, rows = int(values["c"]), int(values["r"])
    if (width, height) == (cols, rows):
        problems.append("board is not supersampled relative to its cell box")
    return problems


def inspect_motion(control: str, pixels: bytes) -> list[str]:
    """The motion layer is RGBA and must be mostly transparent."""
    values = control_values(control)
    width = int(values["s"])
    height = int(values["v"])
    if len(pixels) != width * height * 4:
        return [f"motion layer is {len(pixels)} bytes, not {width}x{height}x4"]
    alphas = pixels[3::4]
    opaque = sum(1 for a in alphas if a > 0)
    if opaque == 0:
        return ["motion layer is empty"]
    if opaque * 2 > len(alphas):
        return [f"motion layer is {100 * opaque // len(alphas)}% opaque; it should be sparse"]
    return []


def spawn_on_pty(binary: str, env: dict[str, str], cols: int, rows: int, px_w: int, px_h: int):
    master, slave = os.openpty()
    winsize = struct.pack("HHHH", rows, cols, px_w, px_h)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, winsize)

    pid = os.fork()
    if pid == 0:  # child
        try:
            os.close(master)
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
            os.dup2(slave, 0)
            os.dup2(slave, 1)
            os.dup2(slave, 2)
            if slave > 2:
                os.close(slave)
            os.execvpe(binary, [binary], env)
        except BaseException as exc:  # pragma: no cover - child error path
            os._exit(127 if exc else 1)
    os.close(slave)
    return pid, master


def drain(fd: int, deadline: float) -> bytes:
    chunks = []
    while time.time() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if not ready:
            continue
        try:
            data = os.read(fd, 65536)
        except OSError as exc:
            if exc.errno == errno.EIO:
                break
            raise
        if not data:
            break
        chunks.append(data)
    return b"".join(chunks)


def apcs(data: bytes) -> list[bytes]:
    spans = []
    i = 0
    while i + 2 < len(data):
        if data[i] == ESC and data[i + 1] == ESC:
            i += 2  # doubled ESC: tmux passthrough payload
            continue
        if data[i] == ESC and data[i + 1] == ord("_") and data[i + 2] == ord("G"):
            end = data.find(b"\x1b\\", i + 3)
            if end < 0:
                break
            spans.append(data[i : end + 2])
            i = end + 2
        else:
            i += 1
    return spans


def split_control(apc: bytes) -> tuple[str, bytes]:
    body = apc[3:-2]
    if b";" in body:
        control, payload = body.split(b";", 1)
        return control.decode("ascii"), payload
    return body.decode("ascii"), b""


def envelopes(data: bytes) -> list[bytes]:
    prefix = b"\x1bPtmux;"
    found = []
    index = 0
    while True:
        start = data.find(prefix, index)
        if start < 0:
            return found
        i = start + len(prefix)
        while i + 1 < len(data):
            if data[i] == ESC:
                if data[i + 1] == ESC:
                    i += 2
                    continue
                found.append(data[start : i + 2])
                index = i + 2
                break
            i += 1
        else:
            return found


def unwrap(envelope: bytes) -> bytes:
    """Turn a `DCS tmux;` envelope back into the APC it carried."""
    payload = envelope[len(b"\x1bPtmux;") :]
    payload = payload[: -2]  # trailing ST of the DCS wrapper
    return payload.replace(b"\x1b\x1b", b"\x1b")


def check(data: bytes, tmux: bool, expect_square_minos: bool = False) -> list[str]:
    problems = []
    if b"\x1b[?1049h" not in data:
        problems.append("never entered the alternate screen")
    if b"\x1b[?1049l" not in data:
        problems.append("never left the alternate screen")

    if tmux:
        envelopes_found = envelopes(data)
        if not envelopes_found:
            problems.append("no DCS passthrough envelope emitted")
        if apcs(data):
            problems.append("bare APC survived in tmux mode")
        found = [unwrap(envelope) for envelope in envelopes_found]
    else:
        found = apcs(data)
        if envelopes(data):
            problems.append("unexpected passthrough envelope outside tmux")

    if not found:
        problems.append("no Kitty graphics command emitted")
        return problems

    transmits = 0
    formats = set()
    previews = set()
    for apc in found:
        if not apc.startswith(b"\x1b_G") or not apc.endswith(b"\x1b\\"):
            problems.append(f"malformed APC: {apc[:40]!r}")
            continue
        control, payload = split_control(apc)
        if "q=2" not in control:
            problems.append(f"command is not quiet: {control}")
        if control.startswith("a=d"):
            if payload:
                problems.append("delete command carried a payload")
            continue
        if "m=" not in control:
            problems.append(f"chunk has no chunking key: {control}")
            continue
        if control.startswith("a=T"):
            transmits += 1
            values = control_values(control)
            required = ["f", "o", "i", "p", "C", "z", "s", "v"]
            if "c" in values or "r" in values:
                required += ["c", "r"]
            for key in required:
                if key not in values:
                    problems.append(f"missing {key} in {control}")
            if values.get("o") != "z":
                problems.append("payload is not zlib compressed")
            if values.get("C") != "1":
                problems.append("placement moves the cursor")
            # The frame must hug a square mino grid: the image has to divide
            # evenly into its cell box, otherwise the terminal resamples it.
            cells_across = int(values.get("c", "0"))
            cells_down = int(values.get("r", "0"))
            image_w = int(values.get("s", "0"))
            image_h = int(values.get("v", "0"))
            if cells_across and cells_down and "c" in values:
                # The frame hugs a square mino grid, so the image is sent at the
                # terminal's own device resolution: no resampling, square bands.
                cell_w = image_w / cells_across
                cell_h = image_h / cells_down
                if not (4.0 <= cell_w <= 40.0 and 4.0 <= cell_h <= 40.0):
                    problems.append(
                        f"implausible cell size {cell_w:.2f}x{cell_h:.2f}px"
                    )
                band_w, band_h = image_w // 10, image_h // 20
                if abs(band_w - band_h) > 1:
                    problems.append(
                        f"mino bands are not square: {band_w}x{band_h}px"
                    )
            if values.get("f") == "24":
                formats.add("rgb")
            elif values.get("f") == "32":
                formats.add("rgba")
            else:
                problems.append(f"unexpected pixel format {values.get('f')}")
            # Well layers carry the cell box (`c`/`r`); the HUD previews are
            # native-size sprites placed on their own.
            is_well_layer = "c" in values and "r" in values
            if not is_well_layer:
                previews.add(int(values.get("i", "0")))
            z_index = int(values.get("z", "0"))
            if z_index not in (0, 1):
                problems.append(f"unexpected z index {z_index}")
            if len(payload) > 4096:
                problems.append(f"chunk exceeds 4096 base64 bytes: {len(payload)}")
            if len(payload) % 4:
                problems.append("chunk splits a base64 quartet")
            try:
                decoded = base64.b64decode(payload, validate=True)
            except Exception:
                problems.append("payload is not valid base64")
                continue
            if decoded[:1] != b"\x78":
                problems.append("payload is not a zlib stream")
                continue
            if "m=0" in control:
                channels = 4 if values.get("f") == "32" else 3
                pixels = zlib.decompress(decoded)
                if "c" not in values:
                    # HUD preview: every block must be a multiple of the preview
                    # mino, which is a hard-coded 10x10 square. The HOLD slot is
                    # legitimately empty at the start of a game, the NEXT strip
                    # never is, so only that one must contain pieces.
                    # The strip is four minos wide, so the mino side follows
                    # from the image width.
                    side = int(values["s"]) // 4
                    if side < 4:
                        problems.append(f"preview strip is only {values['s']}px wide")
                    else:
                        problems.extend(
                            check_preview_minos(
                                control, pixels, side, int(values["v"]) > 40
                            )
                        )
                elif channels == 3:
                    problems.extend(inspect_board(control, pixels))
                else:
                    problems.extend(inspect_motion(control, pixels))
                    if expect_square_minos:
                        problems.extend(
                            check_square_minos(control, pixels, channels)
                        )
    if transmits < 1:
        problems.append("no transmit-and-place command")
    if expect_square_minos and "rgba" not in formats:
        problems.append("no motion layer to measure mino shapes")
    if len(previews) != 2:
        problems.append(f"expected two HUD preview layers, saw {sorted(previews)}")

    if b"Gi=" in data:
        problems.append("terminal response leaked into the output stream")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/debug/tui-tetris")
    parser.add_argument("--tmux", action="store_true", help="wrap in passthrough mode")
    parser.add_argument(
        "--graphics",
        default="auto",
        choices=["auto", "kitty", "off"],
        help="auto leaves TUI_TETRIS_GRAPHICS unset (the shipping default)",
    )
    parser.add_argument("--frames-ms", type=int, default=900)
    parser.add_argument(
        "--expect-square-minos",
        action="store_true",
        help="decode the image and assert every mino block is square",
    )
    parser.add_argument("--cols", type=int, default=80)
    parser.add_argument("--rows", type=int, default=24)
    parser.add_argument("--px-w", type=int, default=640)
    parser.add_argument("--px-h", type=int, default=408)
    args = parser.parse_args()

    env = dict(os.environ)
    env.pop("TUI_TETRIS_GRAPHICS", None)
    if args.graphics != "auto":
        env["TUI_TETRIS_GRAPHICS"] = args.graphics
    env["TUI_TETRIS_HEADLESS"] = "0"
    env["TETRIS_AI_DISABLED"] = "1"
    env.pop("TMUX", None)
    env.pop("STY", None)
    if args.tmux:
        # Emulate Ghostty behind tmux: the pane reports tmux, the outer
        # terminal identity survives only in the GHOSTTY_* paths.
        env["TERM"] = "screen-256color"
        env["TERM_PROGRAM"] = "tmux"
        env["TMUX"] = "/tmp/tmux-smoke,1,0"
        # Emulate a pane whose outer terminal is Ghostty with passthrough on,
        # instead of depending on this machine's tmux server settings.
        env["TUI_TETRIS_TMUX_CLIENT"] = "xterm-ghostty"
        env["TUI_TETRIS_TMUX_PASSTHROUGH"] = "on"
        env.pop("GHOSTTY_RESOURCES_DIR", None)
    else:
        # A plain kitty-graphics terminal, so detection does not treat the pane
        # as a multiplexer.
        env["TERM"] = "xterm-ghostty"
        env["TERM_PROGRAM"] = "ghostty"
        env.pop("GHOSTTY_RESOURCES_DIR", None)
        env.pop("TUI_TETRIS_TMUX_CLIENT", None)
        env.pop("TUI_TETRIS_TMUX_PASSTHROUGH", None)

    pid, master = spawn_on_pty(args.binary, env, args.cols, args.rows, args.px_w, args.px_h)
    try:
        data = drain(master, time.time() + args.frames_ms / 1000.0)
        try:
            os.write(master, b"q")
        except OSError:
            # The process may already be gone (e.g. it refused to start).
            pass
        data += drain(master, time.time() + 1.5)
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        os.waitpid(pid, 0)
        os.close(master)

    if args.graphics == "off":
        # The playfield is graphics-only, so refusing to start is the contract.
        if b"\x1b_G" in data:
            print("FAIL: graphics escapes emitted while disabled")
            return 1
        if b"graphics" not in data or b"alternate screen" in data:
            print("FAIL: expected a clear refusal mentioning graphics")
            print(data.decode("utf-8", "replace")[:400])
            return 1
        print(f"OK: refused to start without graphics ({len(data)} bytes)")
        return 0

    problems = check(data, args.tmux, args.expect_square_minos)
    if problems:
        for problem in problems:
            print(f"FAIL: {problem}")
        return 1
    unwrapped = [unwrap(e) for e in envelopes(data)] if args.tmux else apcs(data)
    print(
        f"OK: {len(unwrapped)} graphics chunk(s), "
        f"{len(envelopes(data))} passthrough envelope(s), {len(data)} bytes"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
