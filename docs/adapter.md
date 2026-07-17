# Tetris AI Adapter Standard (v2.1.0)

This is the **single source of truth** for:
- the wire protocol between a Tetris game adapter (server) and `tetris-ai` (client)
- deterministic controller/observer rules (no ambiguity, no “trial-and-error”)
- the fixed acceptance / release gate for any game that wants to integrate

Transport is intentionally thin: the protocol is defined in terms of **JSON messages**; the only supported transport in this project is **TCP localhost**.

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are
normative. Sections explicitly marked non-normative provide client guidance only.

## 0) Defaults (MUST)
- Address: `127.0.0.1:7777`
- Framing: **line-delimited JSON** (exactly one JSON object per line)
- Protocol version: `2.1.0` (semver)
- Observation frequency: 20Hz by default, configurable from 1–60Hz
- Maximum accepted actions in one action-mode command: 32

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

The adapter maintains **at most one controller** at a time. There MAY be no
controller before the first eligible handshake or after an explicit release.
- Only the controller may send `command`.
- Observers may receive `observation` but any `command` from an observer MUST be rejected with `error.code="not_controller"`.

### 2.1 Role negotiation (no ambiguity)

The client MAY include `requested.role` in `hello`:
- `"auto"`: adapter uses its default controller policy.
- `"controller"`: client prefers to be controller (adapter may refuse if another controller is active).
- `"observer"`: client must not become controller as a side-effect of `hello`.

If the adapter does not implement role negotiation, it MUST ignore `requested.role` (backward compatible).

The tui-tetris adapter implements role negotiation. With no active controller,
the first handshaken client whose requested role is `"auto"` or `"controller"`
becomes controller; a client requesting `"observer"` remains observer.

The adapter SHOULD include in `welcome`:
- `client_id` (stable per connection)
- `role` (`"controller"` or `"observer"`) as assigned to this connection
- `controller_id` (the currently active controller’s id; may equal `client_id`)

### 2.2 `control(action="claim")` (MUST be idempotent)

`claim` exists for “recover control without reconnect” (e.g. after a `release`).

The adapter MUST implement:
- If the caller is already controller: reply `ack(status="ok")`.
- If there is no controller assigned: assign caller as controller and reply `ack(status="ok")`.
- If a different controller is active: reply `error(code="controller_active")`.

### 2.3 `control(action="release")`
- Only the controller may release; otherwise `error(code="not_controller")`.
- On success: reply `ack(status="ok")` and clear controller assignment.
- After release:
  - the adapter MAY immediately auto-promote an eligible connected client OR require an explicit claim
  - whichever policy is chosen MUST be stable and documented (capability flag recommended)

The tui-tetris policy leaves the controller unassigned after `release`; a client
must explicitly `claim`. Automatic promotion applies only when a controller
disconnects.

### 2.4 Controller promotion on disconnect (RECOMMENDED)
If the current controller disconnects, the adapter SHOULD promote the next eligible connected client to controller.

Lifecycle correctness (MUST):
- Treat abrupt disconnects / socket read errors as disconnects for controller cleanup.
- `controller_active` MUST only be returned when a controller is actually still connected/assigned.

Policy visibility (MUST):
- The adapter MUST expose controller policy in `welcome.capabilities.control_policy`.
- `control_policy` fields:
  - `auto_promote_on_disconnect` (boolean)
  - `promotion_order` (string; e.g. `"lowest_client_id"`)

Eligibility rule (MUST):
- Clients that connected with `requested.role="observer"` MUST NOT be auto-promoted to controller on disconnect.
- Such clients may still become controller via explicit `control(action="claim")`.
- `Eligible` in this section means connected clients that are not observer-locked by `requested.role="observer"`.

`welcome.role` and `welcome.controller_id` describe assignment at handshake time.
Protocol v2.1 has no separate role-change event; after a disconnect promotion, a
client observes its effective authorization through subsequent command/control
acknowledgements or errors.

## 3) Sequencing & Framing (MUST)

### 3.1 Framing
- Every message is a single JSON object encoded as UTF-8, terminated with `\n`.
- Empty lines MUST be ignored or rejected as `invalid_command` (implementation-defined), but MUST NOT be treated as a valid message.
- tui-tetris accepts at most 65,536 payload bytes per inbound line (excluding the
  newline terminator). It closes connections that exceed this limit, preventing
  an unterminated frame from growing server memory without bound.

### 3.2 `seq` rules
- `hello.seq` MUST be `1`.
- After a successful `hello → welcome`, the client MUST use strictly increasing
  `seq` values for every subsequent `command` or `control` on that connection.
- Duplicate or decreasing client `seq` MUST return `error.code="invalid_command"`
  and MUST NOT enqueue/apply the message.
- Server message sequences use message-specific correlation:
  - `welcome.seq` echoes `hello.seq`.
  - `ack.seq` and `error.seq` echo the triggering client message when available;
    an unparseable message may produce `error.seq=0`.
  - `observation.seq` belongs to an independent, monotonically increasing
    observation stream and MUST NOT be compared with ack/error sequences.
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

`formats` MUST contain `"json"`. `requested.command_mode` declares the client's
preferred command mode; capabilities in `welcome` remain authoritative.

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

`capabilities.features` is the compatibility union of `features_always` and
`features_optional`. New clients SHOULD use the two explicit lists.

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

The tui-tetris serializer omits an optional field when its value is unavailable;
clients SHOULD also tolerate explicit `null` for forward-compatible decoding.
`board_id` changes only when locked board cells change. `state_hash` is an opaque,
16-character lowercase hexadecimal digest of the documented snapshot state and
MUST NOT be assumed stable across protocol/ruleset versions.

Example:
```json
{"type":"observation","seq":42,"ts":1730000001200,"playable":true,"paused":false,"game_over":false,"episode_id":0,"seed":1,"piece_id":12,"step_in_piece":0,"board":{"width":10,"height":20,"cells":[[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0]]},"board_id":123,"active":{"kind":"t","rotation":"north","x":4,"y":0},"ghost_y":17,"next":"i","next_queue":["i","o","t","s","z"],"can_hold":true,"last_event":{"locked":true,"lines_cleared":2,"line_clear_score":1200,"tspin":"full","combo":1,"back_to_back":true},"state_hash":"e1bca4d1b673b8c2","score":1200,"level":2,"lines":17,"timers":{"drop_ms":320,"lock_ms":120,"line_clear_ms":0}}
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

`actions` MAY be empty and MUST contain no more than 32 entries. If `restart` is
present, `actions` MUST include `"restart"`; `restart.seed` is an unsigned 32-bit
integer. The adapter acknowledges only after the fixed-step game loop applies the
command.

Restart with fixed seed (for deterministic evaluation/training):
```json
{"type":"command","seq":7,"ts":1730000001300,"mode":"action","actions":["restart"],"restart":{"seed":123}}
```

Place mode:
```json
{"type":"command","seq":8,"ts":1730000001300,"mode":"place","place":{"x":3,"rotation":"east","useHold":false}}
```

`place.x` is the tetromino origin, not necessarily the leftmost occupied mino.
Geometrically invalid or unreachable placements return `invalid_place`.
Place application is atomic: an unsuccessful command leaves board, active piece,
hold/queue state, timers, and score unchanged.

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
`playable` describes game lifecycle, not client authorization:

- `playable=true` means the game is neither paused nor game-over and gameplay can
  progress for an authorized controller.
- A client MUST still be the active controller before sending commands.
- Observers may therefore receive `playable=true` observations but remain unable
  to command the game.

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

`snapshot_required` is reserved for adapters that need a fresh snapshot before
mapping a high-level placement. The current tui-tetris adapter applies place
commands directly to authoritative core state and does not emit this code.

## 7) Observation Frequency (SHOULD)
- Adapters may emit observations every fixed step or at a throttled interval (e.g. 20Hz).
- If throttled, these transitions SHOULD trigger an immediate observation:
  - `piece_id` changes / piece spawn
  - `last_event.locked=true`
  - `paused` changes
  - `game_over` changes

The tui-tetris adapter uses `TETRIS_AI_OBS_HZ` (default `20`, clamped to `1..60`).
It emits observations only to clients that requested streaming and skips periodic
observation construction when no streaming subscribers exist. A streaming hello
requests an immediate full snapshot. Periodic scheduling uses a fixed-step phase
accumulator, preserving the requested long-run frequency without integer-period
drift (for example, 20Hz does not degrade to one frame every four 16ms steps).

### 7.1 Runtime configuration (tui-tetris)

| Variable | Default | Meaning |
| --- | --- | --- |
| `TETRIS_AI_HOST` | `127.0.0.1` | TCP bind host |
| `TETRIS_AI_PORT` | `7777` | TCP bind port; `0` selects an ephemeral port in tests |
| `TETRIS_AI_DISABLED` | unset | `1` or `true` disables the adapter |
| `TETRIS_AI_MAX_PENDING` | `10` | Bounded inbound command queue capacity |
| `TETRIS_AI_OBS_HZ` | `20` | Observation frequency, clamped to `1..60` |
| `TETRIS_AI_LOG_PATH` | unset | Optional newline-delimited wire log path |
| `TETRIS_AI_LOG_EVERY_N` | `1` | Log every Nth wire record; values below 1 use the default |
| `TETRIS_AI_LOG_MAX_LINES` | unlimited | Optional maximum number of written log lines |

Wire logging is diagnostic and MUST NOT change protocol ordering or game state.

## 8) Acceptance / Release Gate (MUST)

An adapter is **ACCEPTED** only if it passes all items below.

### 8.1 Protocol correctness
- handshake required enforcement (`handshake_required`)
- protocol major mismatch enforcement (`protocol_mismatch`)
- deterministic control semantics:
  - observer never becomes controller when `requested.role="observer"`
  - observer is not auto-promoted on controller disconnect
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

### 8.5 Client Consumption Best Practices (Non-Normative)

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

The maintained verification client is `scripts/adapter_verify.py`. Keeping socket
buffering and lifecycle assertions in an executable script prevents documentation
examples from silently diverging from the acceptance suite. It uses only the
Python standard library.

Start tui-tetris in another terminal, then run:

```bash
# Run ready/full-snapshot, idempotent claim, restart, and fixed-seed checks
python3 scripts/adapter_verify.py all

# Run one check or target a non-default endpoint
python3 scripts/adapter_verify.py ready
python3 scripts/adapter_verify.py determinism --host 127.0.0.1 --port 7777 --seed 123 --pieces 8
```

Checks fail with exit status `2` and a diagnostic. The determinism check restarts
two independent connections with the same seed, advances pieces with `hardDrop`,
and compares the resulting `next_queue` sequence.

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
        "formats": { "type": "array", "items": { "type": "string", "enum": ["json"] } },
        "command_modes": { "type": "array", "items": { "type": "string", "enum": ["action", "place"] } },
        "features": { "type": "array", "items": { "$ref": "#/definitions/capability_feature" } },
        "features_always": { "type": "array", "items": { "$ref": "#/definitions/capability_feature" } },
        "features_optional": { "type": "array", "items": { "$ref": "#/definitions/capability_feature" } },
        "control_policy": {
          "type": "object",
          "properties": {
            "auto_promote_on_disconnect": { "type": "boolean" },
            "promotion_order": { "type": "string", "enum": ["lowest_client_id"] }
          },
          "required": ["auto_promote_on_disconnect", "promotion_order"]
        }
      },
      "required": ["formats", "command_modes", "features", "features_always", "features_optional", "control_policy"]
    },
    "capability_feature": { "type": "string", "enum": ["hold","next","next_queue","can_hold","ghost_y","board_id","last_event","state_hash","score","timers"] },
    "piece_kind": { "type": "string", "enum": ["i","o","t","s","z","j","l"] },
    "rotation": { "type": "string", "enum": ["north","east","south","west"] },
    "action_name": { "type": "string", "enum": ["moveLeft","moveRight","softDrop","hardDrop","rotateCw","rotateCcw","hold","pause","restart"] },
    "tspin": { "type": "string", "enum": ["mini","full"] },
    "role": { "type": "string", "enum": ["auto","controller","observer"] },
    "place": {
      "type": "object",
      "properties": {
        "x": { "type": "integer", "minimum": -128, "maximum": 127 },
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
        "x": { "type": "integer", "minimum": -128, "maximum": 127 },
        "y": { "type": "integer", "minimum": -128, "maximum": 127 }
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
        "protocol_version": { "type": "string", "pattern": "^2\\.[0-9]+\\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\\+[0-9A-Za-z.-]+)?$" },
        "formats": { "type": "array", "minItems": 1, "contains": { "const": "json" }, "items": { "type": "string" } },
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
        "protocol_version": { "type": "string", "pattern": "^2\\.[0-9]+\\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\\+[0-9A-Za-z.-]+)?$" },
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
            "actions": { "type": "array", "maxItems": 32, "items": { "$ref": "#/definitions/action_name" } },
            "restart": {
              "type": "object",
              "properties": {
                "seed": { "type": "integer", "minimum": 0, "maximum": 4294967295 }
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
        "state_hash": { "type": "string", "pattern": "^[0-9a-f]{16}$" },
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
        "status": { "type": "string", "enum": ["ok"] }
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
