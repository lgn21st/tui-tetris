# Tetris AI Adapter Standard (v2.1.0)

This is the **single source of truth** for:
- the wire protocol between a Tetris game adapter (server) and `tetris-ai` (client)
- deterministic controller/observer rules (no ambiguity, no “trial-and-error”)
- the fixed acceptance / release gate for any game that wants to integrate

Transport is intentionally thin: the protocol is defined in terms of **JSON messages**; the only supported transport in this project is **TCP localhost**.

## 0) Defaults (MUST)
- Address: `127.0.0.1:7777`
- Framing: **line-delimited JSON** (exactly one JSON object per line)
- Protocol version: `2.1.0` (semver)

## 1) Compatibility Scope

### MUST support
- `hello → welcome` handshake with version checking (major version compatible with `2.x`)
- deterministic controller/observer model (Section 2)
- `command`:
  - `mode="action"` (at least: `restart`, `pause`, movement/rotation/drop; `hold` if supported)
  - `mode="place"` (high-level placement interface)
- streaming `observation` snapshots (Section 5)
- deterministic tick ordering on the game side: **apply commands → tick rules → emit snapshot**

### MUST NOT depend on
- screen scraping
- OS-level input injection
- non-deterministic UI side-effects mutating core rules

## 2) Roles & Control (Deterministic, Normative)

The adapter maintains **exactly one controller** at a time.
- Only the controller may send `command`.
- Observers may receive `observation` but any `command` from an observer MUST be rejected with `error.code="not_controller"`.

### 2.1 Role negotiation (no ambiguity)

The client MAY include `requested.role` in `hello`:
- `"auto"`: adapter uses its default controller policy.
- `"controller"`: client prefers to be controller (adapter may refuse if another controller is active).
- `"observer"`: client must not become controller as a side-effect of `hello`.

If the adapter does not implement role negotiation, it MUST ignore `requested.role` (backward compatible).

The adapter SHOULD include in `welcome`:
- `client_id` (stable per connection)
- `role` (`"controller"` or `"observer"`) as assigned to this connection
- `controller_id` (the currently active controller’s id; may equal `client_id`)

### 2.2 `control(action="claim")` (MUST be idempotent)

`claim` exists for “recover control without reconnect” (e.g. after a `release`).

The adapter MUST implement:
- If the caller is already controller: reply `ack(status="ok")`.
- If there is no controller assigned: assign caller as controller and reply `ack(status="ok")`.
- If a different controller is active: reply `error(code="controller_active")` and SHOULD include `controller_id`.

### 2.3 `control(action="release")`
- Only the controller may release; otherwise `error(code="not_controller")`.
- On success: reply `ack(status="ok")` and clear controller assignment.
- After release:
  - the adapter MAY immediately auto-promote an observer OR require an explicit claim
  - whichever policy is chosen MUST be stable and documented (capability flag recommended)

### 2.4 Controller promotion on disconnect (RECOMMENDED)
If the current controller disconnects, the adapter SHOULD promote the next available connected client to controller.

Lifecycle correctness (MUST):
- Treat abrupt disconnects / socket read errors as disconnects for controller cleanup.
- `controller_active` MUST only be returned when a controller is actually still connected/assigned.

Policy visibility (MUST):
- The adapter MUST expose controller policy in `welcome.capabilities.control_policy`.
- `control_policy` fields:
  - `auto_promote_on_disconnect` (boolean)
  - `promotion_order` (string; e.g. `"lowest_client_id"`)

## 3) Sequencing & Framing (MUST)

### 3.1 Framing
- Every message is a single JSON object encoded as UTF-8, terminated with `\n`.
- Empty lines MUST be ignored or rejected as `invalid_command` (implementation-defined), but MUST NOT be treated as a valid message.

### 3.2 `seq` rules
- `hello.seq` MUST be `1`.
- After a successful `hello → welcome`, each sender MUST use strictly increasing `seq` values.
- Duplicate or decreasing `seq` MUST return `error.code="invalid_command"` and MUST NOT enqueue/apply the message.
- Backpressure retry rule:
  - If a `command` is rejected with `error.code="backpressure"`, treat it as **not enqueued** and retry using a **new, larger `seq`**.
  - Server SHOULD include `error.retry_after_ms` to guide client retry pacing.

## 4) Message Types (Wire Format)

All messages include:
- `type` (string)
- `seq` (integer)
- `ts` (unix ms, integer)

### 4.1 hello (client → game)
Required fields: `type, seq, ts, client, protocol_version, formats, requested`

Example:
```json
{"type":"hello","seq":1,"ts":1738291200000,"client":{"name":"tetris-ai","version":"0.1.0"},"protocol_version":"2.1.0","formats":["json"],"requested":{"stream_observations":true,"command_mode":"place","role":"auto"}}
```

### 4.2 welcome (game → client)
Required fields: `type, seq, ts, protocol_version, game_id, capabilities, client_id, role, controller_id`

Deterministic control fields (MUST):
- `client_id` (stable per connection; unique among concurrently connected clients)
- `role` (`"controller"` or `"observer"`) as assigned to this connection
- `controller_id` (the currently active controller’s id; MAY equal `client_id`; MUST be `null` if no controller exists)

Example:
```json
{"type":"welcome","seq":1,"ts":1738291200100,"protocol_version":"2.1.0","client_id":1,"role":"controller","controller_id":1,"game_id":"your-game","capabilities":{"formats":["json"],"command_modes":["action","place"],"features":["hold","next","next_queue","can_hold","ghost_y","board_id","last_event","state_hash","score","timers"],"features_always":["next","next_queue","can_hold","board_id","state_hash","score","timers"],"features_optional":["hold","ghost_y","last_event"],"control_policy":{"auto_promote_on_disconnect":true,"promotion_order":"lowest_client_id"}}}
```

### 4.3 observation (game → client)

The observation is a **full snapshot** (not a delta). For training/inference, this project assumes:
- fixed board shape: `width=10`, `height=20`
- stable encoding for `cells`: `0` empty; `1..7` map to `I,O,T,S,Z,J,L`
- `next_queue` is length 5 and `next == next_queue[0]`

Required fields in every observation:
- `type, seq, ts`
- `playable, paused, game_over`
- `episode_id, seed, piece_id, step_in_piece`
- `board, board_id`
- `next, next_queue`
- `can_hold`
- `state_hash`
- `score, level, lines`
- `timers`

`active`:
- when `playable=true`, `active` MUST be present
- when `playable=false`, `active` MAY be omitted

Optional fields:
- `ghost_y`, `hold`, `last_event`

Example:
```json
{"type":"observation","seq":42,"ts":1730000001200,"playable":true,"paused":false,"game_over":false,"episode_id":0,"seed":1,"piece_id":12,"step_in_piece":0,"board":{"width":10,"height":20,"cells":[[0,0,0,0,0,0,0,0,0,0]]},"board_id":123,"active":{"kind":"t","rotation":"north","x":4,"y":0},"ghost_y":17,"next":"i","next_queue":["i","o","t","s","z"],"hold":null,"can_hold":true,"last_event":{"locked":true,"lines_cleared":2,"line_clear_score":1200,"tspin":"full","combo":1,"back_to_back":true},"state_hash":"e1bca4d1b673b8c2","score":1200,"level":2,"lines":17,"timers":{"drop_ms":320,"lock_ms":120,"line_clear_ms":0}}
```

#### last_event semantics (recommended)
`last_event` is emitted when an event occurred (typically on lock/line clear); otherwise it is omitted.
- `combo` is an integer and may be negative; `-1` means “no active combo chain”.

### 4.4 command (client → game)

Two modes:

Action mode:
```json
{"type":"command","seq":7,"ts":1730000001300,"mode":"action","actions":["rotateCw","moveLeft","hardDrop"]}
```

Restart with fixed seed (for deterministic evaluation/training):
```json
{"type":"command","seq":7,"ts":1730000001300,"mode":"action","actions":["restart"],"restart":{"seed":123}}
```

Place mode:
```json
{"type":"command","seq":8,"ts":1730000001300,"mode":"place","place":{"x":3,"rotation":"east","useHold":false}}
```

### 4.5 control (client → game)
```json
{"type":"control","seq":9,"ts":1730000001350,"action":"claim"}
```

### 4.6 ack (either direction; commonly game → client)
```json
{"type":"ack","seq":7,"ts":1730000001310,"status":"ok"}
```

### 4.7 error (either direction)
```json
{"type":"error","seq":7,"ts":1730000001310,"code":"invalid_command","message":"Unknown action: spin"}
```

Backpressure hint (optional):
```json
{"type":"error","seq":8,"ts":1730000001311,"code":"backpressure","message":"Command queue full","retry_after_ms":50}
```

## 5) Lifecycle & Playability (MUST)

### 5.1 `playable`
`playable=true` MUST mean: the controller can send commands and the game will progress.

If `paused=true` and the game is not accepting commands that advance the game:
- `playable` SHOULD be `false`.

### 5.2 restart
When the controller sends `restart`:
- the adapter MUST reply `ack(status="ok")` (or a well-typed `error` if impossible)
- within a short window (recommended ≤ 2s), observations MUST reflect a fresh episode:
  - `game_over=false`, `paused=false`, `playable=true`
  - board reset (or equivalent “new episode” state)

#### 5.2.1 restart seed (MUST for training determinism)
For training/evaluation, the controller MAY request a deterministic restart seed:
```json
{"type":"command","seq":123,"ts":1730000001300,"mode":"action","actions":["restart"],"restart":{"seed":123}}
```

Semantics (MUST):
- If `restart.seed` is present:
  - the adapter MUST start a fresh episode whose **entire RNG stream** is derived from that seed (at minimum: the piece sequence / bag shuffle).
  - the first observation of the new episode MUST report `seed` equal to the requested seed.
  - for a given adapter implementation + ruleset, the piece sequence MUST be **bit-for-bit identical** for the same seed.
- If `restart.seed` is omitted:
  - the adapter MAY pick any seed; the chosen seed MUST be reported in observations (`observation.seed`) for reproducibility.

Notes:
- This seed mechanism exists for ML determinism and is not required for normal gameplay UX.
- If the adapter has any other randomness (visual effects, jitter, etc.), it SHOULD also be derived from the same seed so replay/eval is stable.

### 5.3 pause
If `pause` exists:
- it toggles or sets `paused=true` deterministically
- while paused:
  - observations must keep streaming (even if reduced rate)
  - `playable` must reflect whether the policy loop should proceed
  - non-lifecycle actions (move/rotate/drop/hold) SHOULD be ignored

## 6) Error Codes (MUST)
- `handshake_required`: command/control before hello
- `protocol_mismatch`: incompatible protocol major version
- `not_controller`: non-controller sent command/release
- `controller_active`: claim while a different controller exists
- `invalid_command`: JSON parse/shape errors, unknown message type, or bad sequencing
- `invalid_place`: place command could not be mapped/applied
- `hold_unavailable`: hold requested when unavailable
- `snapshot_required`: snapshot required for mapping/applying
- `backpressure`: command queue full
  - Optional field: `retry_after_ms` (integer, `>=1`) for client retry pacing.

## 7) Observation Frequency (SHOULD)
- Adapters may emit observations every fixed step or at a throttled interval (e.g. 20Hz).
- If throttled, these transitions SHOULD trigger an immediate observation:
  - `piece_id` changes / piece spawn
  - `last_event.locked=true`
  - `paused` changes
  - `game_over` changes

## 8) Acceptance / Release Gate (MUST)

An adapter is **ACCEPTED** only if it passes all items below.

### 8.1 Protocol correctness
- handshake required enforcement (`handshake_required`)
- protocol major mismatch enforcement (`protocol_mismatch`)
- deterministic control semantics:
  - observer never becomes controller when `requested.role="observer"`
  - `claim` idempotence (self-claim returns `ack`)
  - commands from observers return `not_controller`
- sequencing rules and backpressure retry semantics

### 8.2 Snapshot completeness
- first observation after welcome is a full snapshot
- required fields always present (Section 4.3)
- board shape and encoding are correct

### 8.3 Lifecycle
- restart returns to playable state
- pause semantics are deterministic (if supported)

### 8.4 Closed-loop stability
- the adapter can run repeated rounds and the runner can exit cleanly (no hang)
- reconnect works without restarting the game process

## 8.5) Client Consumption Best Practices (Non-Normative)

The following guidance is for robust client implementations. It does not change
wire compatibility requirements.

- Process one TCP stream in-order per connection. Avoid parallel reordering in the consumer.
- Track `max_episode_id_seen`; discard observations whose `episode_id` is lower than that value.
- Treat a new episode as active only after observing both:
  - `episode_id` changed, and
  - `step_in_piece == 1`
- Gate action commands behind lifecycle flags:
  - send move/rotate/drop/hold only when `playable=true && paused=false && game_over=false`.
- On `error.code in {"invalid_place","not_controller"}`:
  - stop sending additional action commands immediately,
  - wait for the next streamed `observation` as the new source-of-truth,
  - recompute policy on that snapshot before sending further actions.

This pattern prevents stale in-flight observations around restart boundaries from
causing false commands, while keeping protocol semantics unchanged.

## 9) Self-contained Verification (python3 stdlib only)

All examples assume `127.0.0.1:7777`.

### 9.1 Ready probe (welcome + first observation)
```bash
python3 - <<'PY'
import json, socket, time, sys
host, port = "127.0.0.1", 7777
timeout_s = 2.0

def read_line(sock):
    sock.settimeout(timeout_s)
    buf = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            return None
        buf += chunk
        if b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            return json.loads(line.decode("utf-8", errors="replace"))

sock = socket.create_connection((host, port), timeout=timeout_s)
hello = {
  "type":"hello","seq":1,"ts":int(time.time()*1000),
  "client":{"name":"acceptance","version":"0.1.0"},
  "protocol_version":"2.1.0","formats":["json"],
  "requested":{"stream_observations":True,"command_mode":"place","role":"observer"},
}
sock.sendall((json.dumps(hello)+"\n").encode())
welcome = read_line(sock)
if not isinstance(welcome, dict) or welcome.get("type") != "welcome":
    print("FAIL: no welcome")
    sys.exit(2)
obs = read_line(sock)
if not isinstance(obs, dict) or obs.get("type") != "observation":
    print("FAIL: no observation")
    sys.exit(2)
print("OK: ready", f"tcp://{host}:{port}", "playable=" + str(bool(obs.get("playable"))))
PY
```

### 9.2 claim idempotence (self-claim must ack)
```bash
python3 - <<'PY'
import json, socket, time, sys
host, port = "127.0.0.1", 7777
timeout_s = 2.0

def send(sock, obj):
    sock.sendall((json.dumps(obj) + "\n").encode("utf-8"))

def recv(sock):
    sock.settimeout(timeout_s)
    buf = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            return None
        buf += chunk
        if b"\n" in buf:
            line, _ = buf.split(b"\n", 1)
            return json.loads(line.decode("utf-8", errors="replace"))

sock = socket.create_connection((host, port), timeout=timeout_s)
send(sock, {"type":"hello","seq":1,"ts":int(time.time()*1000),"client":{"name":"acceptance","version":"0.1.0"},"protocol_version":"2.1.0","formats":["json"],"requested":{"stream_observations":True,"command_mode":"action","role":"controller"}})
_ = recv(sock)  # welcome
send(sock, {"type":"control","seq":2,"ts":int(time.time()*1000),"action":"claim"})
resp = recv(sock)
if not isinstance(resp, dict):
    print("FAIL: no response")
    sys.exit(2)
if resp.get("type") == "ack":
    print("OK: claim is idempotent")
    sys.exit(0)
print("FAIL:", resp)
sys.exit(2)
PY
```

### 9.3 Restart (controller only)
```bash
python3 - <<'PY'
import json, socket, time
host, port = "127.0.0.1", 7777
timeout_s = 2.0

def send(sock, obj):
    sock.sendall((json.dumps(obj) + "\n").encode("utf-8"))

sock = socket.create_connection((host, port), timeout=timeout_s)
send(sock, {"type":"hello","seq":1,"ts":int(time.time()*1000),"client":{"name":"acceptance","version":"0.1.0"},"protocol_version":"2.1.0","formats":["json"],"requested":{"stream_observations":True,"command_mode":"action","role":"controller"}})
send(sock, {"type":"control","seq":2,"ts":int(time.time()*1000),"action":"claim"})
send(sock, {"type":"command","seq":3,"ts":int(time.time()*1000),"mode":"action","actions":["restart"]})
print("sent restart")
PY
```

### 9.4 Restart With Seed (determinism)
The adapter MUST produce the exact same piece sequence for the same `restart.seed`.

This check is intentionally lightweight: it compares the first few `next_queue` snapshots after restart.
```bash
python3 - <<'PY'
import json, socket, time

host, port = "127.0.0.1", 7777
timeout_s = 2.0

def send(sock, obj):
    sock.sendall((json.dumps(obj) + "\n").encode("utf-8"))

def recv_json(sock):
    buf = b""
    while b"\n" not in buf:
        buf += sock.recv(65536)
    line, rest = buf.split(b"\n", 1)
    return json.loads(line.decode("utf-8")), rest

def collect_signature(seed: int, n: int = 8):
    sock = socket.create_connection((host, port), timeout=timeout_s)
    sock.settimeout(timeout_s)
    send(sock, {"type":"hello","seq":1,"ts":int(time.time()*1000),"client":{"name":"acceptance","version":"0.1.0"},"protocol_version":"2.1.0","formats":["json"],"requested":{"stream_observations":True,"command_mode":"action","role":"controller"}})

    # Establish baseline episode id from the first observation (pre-restart).
    baseline_episode_id = None
    baseline_seen = False
    while not baseline_seen:
        msg, _ = recv_json(sock)
        if msg.get("type") == "observation":
            baseline_episode_id = msg.get("episode_id")
            baseline_seen = True

    send(sock, {"type":"control","seq":2,"ts":int(time.time()*1000),"action":"claim"})
    send(sock, {"type":"command","seq":3,"ts":int(time.time()*1000),"mode":"action","actions":["restart"],"restart":{"seed":seed}})
    # Depending on timing, the adapter may emit one or more observations that
    # were "in flight" from the pre-restart episode. For determinism checks,
    # wait for a new episode id and the first step of the new piece.
    sig = []
    active_episode_id = None
    while True:
        msg, _ = recv_json(sock)
        if msg.get("type") != "observation":
            continue

        ep = msg.get("episode_id")
        if active_episode_id is None:
            if ep is not None and ep != baseline_episode_id and msg.get("step_in_piece") == 1 and msg.get("seed") == seed:
                active_episode_id = ep
            continue

        if ep != active_episode_id:
            continue

        q = msg.get("next_queue") or []
        sig.append(tuple(q))
        if len(sig) >= n:
            break
    sock.close()
    return sig

seed = 123
a = collect_signature(seed)
b = collect_signature(seed)
assert a == b, {"seed": seed, "a": a, "b": b}
print("ok: deterministic restart.seed")
PY
```

## 10) JSON Schema (Appendix)

The schema below is included inline to avoid external file dependencies.

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "Tetris AI Adapter Protocol",
  "type": "object",
  "oneOf": [
    { "$ref": "#/definitions/hello" },
    { "$ref": "#/definitions/welcome" },
    { "$ref": "#/definitions/command" },
    { "$ref": "#/definitions/control" },
    { "$ref": "#/definitions/observation" },
    { "$ref": "#/definitions/ack" },
    { "$ref": "#/definitions/error" }
  ],
  "definitions": {
    "capabilities": {
      "type": "object",
      "properties": {
        "formats": { "type": "array", "items": { "type": "string" } },
        "command_modes": { "type": "array", "items": { "type": "string" } },
        "features": { "type": "array", "items": { "type": "string" } },
        "features_always": { "type": "array", "items": { "type": "string" } },
        "features_optional": { "type": "array", "items": { "type": "string" } },
        "control_policy": {
          "type": "object",
          "properties": {
            "auto_promote_on_disconnect": { "type": "boolean" },
            "promotion_order": { "type": "string", "enum": ["lowest_client_id"] }
          },
          "required": ["auto_promote_on_disconnect", "promotion_order"]
        }
      },
      "required": ["formats", "command_modes", "features", "control_policy"]
    },
    "piece_kind": { "type": "string", "enum": ["i","o","t","s","z","j","l"] },
    "rotation": { "type": "string", "enum": ["north","east","south","west"] },
    "action_name": { "type": "string", "enum": ["moveLeft","moveRight","softDrop","hardDrop","rotateCw","rotateCcw","hold","pause","restart"] },
    "tspin": { "type": "string", "enum": ["mini","full"] },
    "role": { "type": "string", "enum": ["auto","controller","observer"] },
    "place": {
      "type": "object",
      "properties": {
        "x": { "type": "integer", "minimum": 0, "maximum": 9 },
        "rotation": { "$ref": "#/definitions/rotation" },
        "useHold": { "type": "boolean" }
      },
      "required": ["x", "rotation", "useHold"]
    },
    "board": {
      "type": "object",
      "properties": {
        "width": { "const": 10 },
        "height": { "const": 20 },
        "cells": {
          "type": "array",
          "minItems": 20,
          "maxItems": 20,
          "items": {
            "type": "array",
            "minItems": 10,
            "maxItems": 10,
            "items": { "type": "integer", "minimum": 0, "maximum": 7 }
          }
        }
      },
      "required": ["width", "height", "cells"]
    },
    "active_piece": {
      "type": "object",
      "properties": {
        "kind": { "$ref": "#/definitions/piece_kind" },
        "rotation": { "$ref": "#/definitions/rotation" },
        "x": { "type": "integer", "minimum": 0, "maximum": 9 },
        "y": { "type": "integer", "minimum": -4, "maximum": 23 }
      },
      "required": ["kind", "rotation", "x", "y"]
    },
    "last_event": {
      "type": "object",
      "properties": {
        "locked": { "type": "boolean" },
        "lines_cleared": { "type": "integer", "minimum": 0, "maximum": 4 },
        "line_clear_score": { "type": "integer", "minimum": 0 },
        "tspin": { "oneOf": [ { "$ref": "#/definitions/tspin" }, { "type": "null" } ] },
        "combo": { "type": "integer" },
        "back_to_back": { "type": "boolean" }
      },
      "required": ["locked", "lines_cleared", "line_clear_score", "combo", "back_to_back"]
    },
    "timers": {
      "type": "object",
      "properties": {
        "drop_ms": { "type": "integer", "minimum": 0 },
        "lock_ms": { "type": "integer", "minimum": 0 },
        "line_clear_ms": { "type": "integer", "minimum": 0 }
      },
      "required": ["drop_ms", "lock_ms", "line_clear_ms"]
    },
    "hello": {
      "type": "object",
      "properties": {
        "type": { "const": "hello" },
        "seq": { "const": 1 },
        "ts": { "type": "integer" },
        "client": {
          "type": "object",
          "properties": {
            "name": { "type": "string" },
            "version": { "type": "string" }
          },
          "required": ["name", "version"]
        },
        "protocol_version": { "type": "string" },
        "formats": { "type": "array", "items": { "type": "string" } },
        "requested": {
          "type": "object",
          "properties": {
            "stream_observations": { "type": "boolean" },
            "command_mode": { "type": "string", "enum": ["action", "place"] },
            "role": { "$ref": "#/definitions/role" }
          },
          "required": ["stream_observations", "command_mode"]
        }
      },
      "required": ["type", "seq", "ts", "client", "protocol_version", "formats", "requested"]
    },
    "welcome": {
      "type": "object",
      "properties": {
        "type": { "const": "welcome" },
        "seq": { "const": 1 },
        "ts": { "type": "integer" },
        "protocol_version": { "type": "string" },
        "client_id": { "type": "integer" },
        "role": { "type": "string", "enum": ["controller","observer"] },
        "controller_id": { "anyOf": [{ "type": "integer" }, { "type": "null" }] },
        "game_id": { "type": "string" },
        "capabilities": { "$ref": "#/definitions/capabilities" }
      },
      "required": ["type", "seq", "ts", "protocol_version", "client_id", "role", "controller_id", "game_id", "capabilities"]
    },
    "command": {
      "oneOf": [
        {
          "type": "object",
          "properties": {
            "type": { "const": "command" },
            "seq": { "type": "integer" },
            "ts": { "type": "integer" },
            "mode": { "const": "action" },
            "actions": { "type": "array", "items": { "$ref": "#/definitions/action_name" } },
            "restart": {
              "type": "object",
              "properties": {
                "seed": { "type": "integer", "minimum": 0 }
              },
              "required": ["seed"],
              "additionalProperties": false
            }
          },
          "required": ["type", "seq", "ts", "mode", "actions"],
          "additionalProperties": false
        },
        {
          "type": "object",
          "properties": {
            "type": { "const": "command" },
            "seq": { "type": "integer" },
            "ts": { "type": "integer" },
            "mode": { "const": "place" },
            "place": { "$ref": "#/definitions/place" }
          },
          "required": ["type", "seq", "ts", "mode", "place"],
          "additionalProperties": false
        }
      ]
    },
    "control": {
      "type": "object",
      "properties": {
        "type": { "const": "control" },
        "seq": { "type": "integer", "minimum": 0 },
        "ts": { "type": "integer", "minimum": 0 },
        "action": { "type": "string", "enum": ["claim", "release"] }
      },
      "required": ["type", "seq", "ts", "action"]
    },
    "observation": {
      "type": "object",
      "properties": {
        "type": { "const": "observation" },
        "seq": { "type": "integer", "minimum": 0 },
        "ts": { "type": "integer", "minimum": 0 },
        "playable": { "type": "boolean" },
        "paused": { "type": "boolean" },
        "game_over": { "type": "boolean" },
        "episode_id": { "type": "integer", "minimum": 0 },
        "seed": { "type": "integer", "minimum": 0 },
        "piece_id": { "type": "integer", "minimum": 0 },
        "step_in_piece": { "type": "integer", "minimum": 0 },
        "board": { "$ref": "#/definitions/board" },
        "board_id": { "type": "integer", "minimum": 0 },
        "active": { "anyOf": [ { "$ref": "#/definitions/active_piece" }, { "type": "null" } ] },
        "ghost_y": { "type": ["integer", "null"] },
        "next": { "$ref": "#/definitions/piece_kind" },
        "next_queue": { "type": "array", "minItems": 5, "maxItems": 5, "items": { "$ref": "#/definitions/piece_kind" } },
        "hold": { "anyOf": [ { "$ref": "#/definitions/piece_kind" }, { "type": "null" } ] },
        "can_hold": { "type": "boolean" },
        "last_event": { "anyOf": [ { "$ref": "#/definitions/last_event" }, { "type": "null" } ] },
        "state_hash": { "type": "string" },
        "score": { "type": "integer", "minimum": 0 },
        "level": { "type": "integer", "minimum": 0 },
        "lines": { "type": "integer", "minimum": 0 },
        "timers": { "$ref": "#/definitions/timers" }
      },
      "required": [
        "type","seq","ts","playable","paused","game_over",
        "episode_id","seed","piece_id","step_in_piece",
        "board","board_id","next","next_queue","can_hold","state_hash",
        "score","level","lines","timers"
      ]
    },
    "ack": {
      "type": "object",
      "properties": {
        "type": { "const": "ack" },
        "seq": { "type": "integer" },
        "ts": { "type": "integer" },
        "status": { "type": "string" }
      },
      "required": ["type", "seq", "ts", "status"]
    },
    "error": {
      "type": "object",
      "properties": {
        "type": { "const": "error" },
        "seq": { "type": "integer" },
        "ts": { "type": "integer" },
        "code": {
          "type": "string",
          "enum": [
            "handshake_required","protocol_mismatch","not_controller","controller_active",
            "invalid_command","invalid_place","hold_unavailable","snapshot_required","backpressure"
          ]
        },
        "message": { "type": "string" }
        ,
        "retry_after_ms": { "type": "integer", "minimum": 1 }
      },
      "required": ["type", "seq", "ts", "code", "message"]
    }
  }
}
```
