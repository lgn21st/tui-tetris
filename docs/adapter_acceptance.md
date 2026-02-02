# Game Adapter Acceptance Standard (Protocol v2.0.0)

This document defines the **fixed acceptance criteria** for any Tetris game implementation that wants to integrate via the unified protocol.

It is intended to be:
- A **release gate** for a game adapter (SwiftUI / TUI / headless sim).
- A **developer checklist** for implementing new games.
- A **verification playbook** with concrete commands.

Target defaults:
- Transport: **TCP**
- Address: **127.0.0.1:7777**
- Framing: **line-delimited JSON** (one JSON object per line)
- Protocol: **v2.0.0**

---

## 1) Compatibility Scope

### Must support
- `hello → welcome` handshake with version checking (major version compatible with `2.x`).
- Controller/observer rules + `control(claim|release)` semantics.
- `command`:
  - `mode=action` (at least: `restart`, `pause`, movement/rotation/drop, `hold` if supported)
  - `mode=place` (final placement interface)
- Streaming `observation` snapshots.
- Deterministic rule application order: **apply commands → tick rules → emit snapshot**.

### Must NOT depend on
- Screen scraping
- OS-level input injection
- Non-deterministic UI side-effects mutating core rules

---

## 2) Release Gate: Pass/Fail Criteria

An adapter release is considered **ACCEPTED** only if it passes **all** of:

1) **Protocol handshake + capabilities** (Section 3)
2) **Observation schema completeness** (Section 4)
3) **Lifecycle correctness** (restart/pause/gameover) (Section 5)
4) **Closed-loop stability** (no hangs, clean exits) (Section 6)
5) **Backpressure + error handling** (Section 7)
6) **Determinism and reproducibility (where applicable)** (Section 8)

Score targets are **not** hard acceptance gates for adapter correctness.
Score is used as a **regression signal** once correctness/stability gates pass.

---

## 3) Handshake & Capabilities (MUST)

### 3.1 Handshake ordering
- Any `command`/`control` before `hello` MUST return:
  - `error.code = "handshake_required"`

### 3.2 Version check
- If client sends `hello.protocol_version` with an incompatible major (e.g. `3.0.0`), server MUST return:
  - `error.code = "protocol_mismatch"`
  - `error.seq` matches the request `seq`

### 3.3 welcome payload
`welcome` MUST include:
- `type = "welcome"`
- `seq`, `ts`
- `protocol_version` (string, e.g. `"2.0.0"`)
- `game_id` (string identifying the game)
- `capabilities` object with:
  - `formats` includes `"json"`
  - `command_modes` includes `"place"` (required) and `"action"` (recommended)
  - `features` should truthfully reflect what observations support (e.g. `hold`, `next_queue`, `timers`)

---

## 4) Observation Schema (MUST)

First observation after `welcome` MUST be a full snapshot.

### 4.1 Required top-level fields
- `type="observation"`
- `seq`, `ts`
- `playable` (bool)
- `paused` (bool)
- `game_over` (bool)
- `board` (object)
- `active` (object)
- `score`, `level`, `lines` (numbers)
- `timers` (object; if the game does not model timers, emit zeros consistently)

### 4.2 board
`board` MUST include:
- `width=10`, `height=20`
- `cells`: 2D array `[height][width]` of ints (0 empty, 1 filled or per-cell occupancy)

### 4.3 active
`active` MUST include:
- `kind` (one of I/O/T/S/Z/J/L)
- `rotation` (`north|east|south|west`)
- `x`, `y` (ints; consistent coordinate system across games)

### 4.4 Recommended fields (STRONGLY RECOMMENDED)
These improve training stability and debugging:
- `next` and `next_queue`
- `hold`, `can_hold`
- `last_event` (locked/lines_cleared/line_clear_score/tspin/combo/back_to_back)
- `episode_id`, `seed`, `piece_id`, `step_in_piece`
- `state_hash` (a stable identifier for debugging determinism)

---

## 5) Lifecycle Correctness (MUST)

### 5.1 playable semantics
`playable=true` MUST mean:
- The controller can send `command` messages and the game will progress.

If `paused=true` and the game is not accepting commands that advance the game, then:
- `playable` SHOULD be `false`

### 5.2 restart semantics
When the controller sends `restart`:
- Server MUST reply `ack(status="ok")` (or a well-typed `error` if impossible)
- Within a short time window (recommended ≤ 2s), observations MUST reflect:
  - `game_over=false`
  - `paused=false` (unless the game intentionally starts paused, in which case `playable` must reflect that)
  - `playable=true`
  - board reset (or equivalent “new episode” state)

### 5.3 pause semantics
If `pause` exists:
- `pause` toggles or sets `paused=true` deterministically.
- While paused:
  - Observations must keep streaming (even if reduced rate).
  - `playable` must be consistent with whether the policy loop should proceed.

---

## 6) Closed-loop Stability (MUST)

### 6.1 No-hang exit (critical)
Client tools must be able to terminate runs without waiting indefinitely.

Minimum stability gate:
- Run `3 × 50` rounds closed-loop (any controller client that drives `place` commands) and verify:
  - the runner exits cleanly
  - no leaked/stuck TCP listener
  - reconnect works without restarting the game process

### 6.2 Long run gate (release-level)
- Run `200` rounds (or `50` rounds repeated 4 times) and confirm:
  - no adapter crash
  - no progressive slowdown (memory leak suspicion)
  - no protocol desync

---

## 7) Error Handling + Backpressure (MUST)

### 7.1 Parse/shape errors
Invalid JSON or missing required fields MUST return:
- `error.code = "invalid_command"`

### 7.2 Observer enforcement
Non-controller attempts to send `command` MUST return:
- `error.code = "not_controller"`

### 7.3 Backpressure
If the adapter has a bounded command queue:
- When full, MUST return `error.code = "backpressure"` for additional commands.
- Observations should keep streaming.

---

## 8) Determinism & Reproducibility (SHOULD)

Determinism is not required for a GUI game, but it is strongly recommended for:
- TUI implementations
- Headless simulator used for RL sampling

If determinism is claimed:
- Fixed seeds must reproduce identical `state_hash` sequences under identical action sequences.
- `episode_id` and `seed` should be present and stable.

---

## 9) Verification Commands (no repo dependencies)

All commands assume the default TCP bind: `127.0.0.1:7777`.

Goal: make verification runnable using only tools that are typically present on macOS/Linux (e.g. `nc`, `python3`).

### 9.1 Handshake via netcat (manual)
Open a TCP connection:

```bash
nc 127.0.0.1 7777
```

Paste a `hello` line (timestamps can be any unix-ms int):

```json
{"type":"hello","seq":1,"ts":1738291200000,"client":{"name":"acceptance","version":"0.1.0"},"protocol_version":"2.0.0","formats":["json"],"requested":{"stream_observations":true,"command_mode":"place"}}
```

Expected:
- A single-line `welcome` JSON response
- Followed by continuous `observation` JSON lines (unless throttled)

### 9.2 Programmatic readiness probe (self-contained python3)
This fails unless:
- `welcome` is received
- the first `observation` has `playable=true`

```bash
python3 - <<'PY'
import json, socket, time, sys
host, port = "127.0.0.1", 7777
timeout_s = 1.5

def read_line(sock):
    sock.settimeout(timeout_s)
    buf = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            return None
        buf += chunk
        if b"\n" in buf:
            line, _ = buf.split(b"\n", 1)
            try:
                return json.loads(line.decode("utf-8", errors="replace"))
            except Exception:
                return None

sock = socket.create_connection((host, port), timeout=timeout_s)
hello = {
  "type":"hello","seq":1,"ts":int(time.time()*1000),
  "client":{"name":"acceptance","version":"0.1.0"},
  "protocol_version":"2.0.0","formats":["json"],
  "requested":{"stream_observations":True,"command_mode":"place"}
}
sock.sendall((json.dumps(hello)+"\n").encode())
welcome = read_line(sock)
if not isinstance(welcome, dict) or welcome.get("type") != "welcome":
    print("FAIL: no welcome received")
    sys.exit(2)
obs = read_line(sock)
if not isinstance(obs, dict) or obs.get("type") != "observation":
    print("FAIL: no observation received")
    sys.exit(2)
if not bool(obs.get("playable", False)):
    print("FAIL: observation.playable is false")
    sys.exit(2)
print("OK: ready", f"tcp://{host}:{port}")
PY
```

### 9.3 Restart command (self-contained python3)
Sends `control:claim` then `command(mode=action, actions=["restart"])`.

```bash
python3 - <<'PY'
import json, socket, time
host, port = "127.0.0.1", 7777
timeout_s = 2.0

def send(sock, obj):
    sock.sendall((json.dumps(obj) + "\n").encode("utf-8"))

sock = socket.create_connection((host, port), timeout=timeout_s)
send(sock, {"type":"hello","seq":1,"ts":int(time.time()*1000),"client":{"name":"acceptance","version":"0.1.0"},"protocol_version":"2.0.0","formats":["json"],"requested":{"stream_observations":True,"command_mode":"action"}})
send(sock, {"type":"control","seq":2,"ts":int(time.time()*1000),"action":"claim"})
send(sock, {"type":"command","seq":3,"ts":int(time.time()*1000),"mode":"action","actions":["restart"]})
print("sent restart")
PY
```

### 9.4 Closed-loop stability runner (implementation-specific)
Closed-loop “N rounds” requires a policy/controller implementation. This doc intentionally does not prescribe a specific client.
Acceptance requirement is behavioral:
- it can repeatedly `place` pieces until game over
- it can detect round boundaries
- it exits cleanly with no hangs

---

## 10) Release Report Template

Copy/paste and fill in:

### Adapter under test
- Game: `<swiftui-tetris | tui-tetris | other>`
- Commit/tag: `<...>`
- OS: `<...>`
- Bind: `127.0.0.1:7777`
- Protocol: `2.0.0`

### Checklist (pass/fail)
- Handshake ordering: ✅/❌
- Protocol mismatch error: ✅/❌
- welcome capabilities truthful: ✅/❌
- Observation required fields: ✅/❌
- restart returns to playable: ✅/❌
- 3×10 stability: ✅/❌
- 3×50 stability: ✅/❌
- 200-round run: ✅/❌
- Backpressure behavior: ✅/❌
- Controller/observer enforcement: ✅/❌

### Artifacts
- Progress logs: `<paths>`
- IO logs: `<paths>`
- Crash logs (if any): `<paths>`
