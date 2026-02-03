# Adapter Protocol (Summary)

Transport: line-delimited JSON (one message per line).
Schema: `docs/adapter-protocol.schema.json`.

Note on schema reading: `definitions.command` is a `oneOf` (action vs place).
That means `definitions.command.required` may be empty at the top-level; required fields live in each branch.


## Integration Checklist (Game Adapter)
- Socket lifecycle: start listener on app launch; clean up on shutdown; support reconnect without restart.
- Handshake: enforce `hello` first; validate `protocol_version` major; reply with `welcome` including `game_id` and `capabilities`.
- Controller rules: first `hello` becomes controller; reject command/control from observers with `not_controller`; promote next observer on controller disconnect; support `claim`/`release`.
- Framing: newline-delimited JSON; reject empty/partial frames; reply with `invalid_command` on parse/shape errors.
- Sequencing: `seq` MUST be strictly increasing per sender after `hello` (no duplicates, no decreases). On violation, reply `error.code = "invalid_command"` and do not enqueue/apply the message.
  - Retry semantics: if a `command` is rejected with `backpressure`, treat it as **not enqueued** and retry using a **new, larger `seq`**.

- Timestamps: `ts` in unix ms; keep monotonic but not necessarily synchronized.
- Observations: send full snapshot (board + active + next + hold + score/level/lines/timers) at fixed step or throttled interval; include `playable` gate.
  - Default cadence: 20Hz (50ms) via `TETRIS_AI_OBS_HZ`.
  - Critical events MUST trigger an immediate observation (do not wait for the next timer):
    - `piece_id` changes / piece spawn
    - `last_event.locked = true`
    - `paused` changes
    - `game_over` changes
  - Implementation note: servers may skip building/broadcasting observations when there are no clients currently streaming observations.
- Piece kinds: accept lowercase or uppercase in incoming payloads; emit consistent case in outgoing snapshots.
- Action mode: implement `moveLeft`, `moveRight`, `softDrop`, `hardDrop`, `rotateCw`, `rotateCcw`, `hold`, `pause`, `restart`.
- Place mode: validate `x`, `rotation`, `useHold`; apply before tick; reply `invalid_place` when the placement cannot be applied (rotation blocked, x out-of-bounds, x blocked, not playable, etc.).
- Backpressure: if command queue is full, return `backpressure` and continue streaming observations.
- Determinism: apply commands before `GameState.tick` on each fixed step; do not let rendering/UI mutate core.
- Debugging: optionally enable wire logging via `TETRIS_AI_LOG_PATH` to capture raw adapter traffic.

## Handshake
### hello (client -> game)
Fields: `type`, `seq`, `ts`, `client`, `protocol_version`, `formats`, `requested`.
Example:
```
{"type":"hello","seq":1,"ts":1738291200000,"client":{"name":"tetris-ai","version":"0.1.0"},"protocol_version":"2.0.0","formats":["json"],"requested":{"stream_observations":true,"command_mode":"place"}}
```

### welcome (game -> client)
Fields: `type`, `seq`, `ts`, `protocol_version`, `game_id`, `capabilities`.
Example:
```
{"type":"welcome","seq":1,"ts":1738291200100,"protocol_version":"2.0.0","game_id":"tui-tetris","capabilities":{"formats":["json"],"command_modes":["action","place"],"features":["hold","next","next_queue","can_hold","ghost_y","board_id","last_event","state_hash","score","timers"],"features_always":["next","next_queue","can_hold","board_id","state_hash","score","timers"],"features_optional":["hold","ghost_y","last_event"]}}
```

### Capabilities feature presence
- `capabilities.features` is a legacy union list (backward compatible).
- `capabilities.features_always` are guaranteed to be present in every observation (training can treat them as non-null).
- `capabilities.features_optional` may be omitted when unknown/not-applicable (training must handle missing).

## Commands
### command (client -> game)
- `mode=action`: `actions: ["moveLeft", "rotateCw", ...]`
- `mode=place`: `place: { "x": 3, "rotation": "east", "useHold": false }`
#### Snapshot invariants
- Board shape is fixed: `board.width=10`, `board.height=20`, and `board.cells` is a 20x10 grid.
- Cell encoding: `board.cells[y][x]` is `0..7` (0 empty; 1..7 map to I,O,T,S,Z,J,L).
- `next_queue` is always length 5 and `next == next_queue[0]`.
- Optional field presence:
  - `active` and `ghost_y` are omitted when there is no active piece (for example when not playable).
  - `hold` is omitted when there is no hold piece.
  - `last_event` is emitted only when an event occurred (typically on lock/line clear); otherwise it is omitted.

#### last_event semantics
`last_event` is intended to match swiftui-tetris:
- `locked`: true when the active piece locked this step.
- `lines_cleared`: number of lines cleared by that lock (0..4).
- `line_clear_score`: base clear points for that lock (includes any B2B multiplier; excludes combo bonus, soft/hard drop points).
- `tspin`: `"mini"` / `"full"` when applicable; omitted/null when no T-Spin.
  - Compatibility note: swiftui-tetris reports `tspin` only when `lines_cleared > 0`. A T-Spin with `lines_cleared=0` may still increase `score`, but `last_event.tspin` is omitted and `line_clear_score` remains `0`.
- `combo`: combo index after applying the lock.
  - `-1` means no active combo chain (e.g., after a lock with `lines_cleared=0`).
  - `0` is the first clear in a chain (no combo bonus).
  - `1+` are consecutive clears (combo bonus applies).
- `back_to_back`: whether the current clear qualifies to carry B2B to the next qualifying clear (i.e., it is a Tetris or Full T-Spin with lines).

Notes:
- Commands are acknowledged after they are mapped and applied during the adapter poll tick.
Examples:
```
{"type":"command","seq":2,"ts":1738291200200,"mode":"action","actions":["moveLeft","rotateCw","hardDrop"]}
{"type":"command","seq":3,"ts":1738291200300,"mode":"place","place":{"x":3,"rotation":"east","useHold":false}}
```

### ack (game -> client)
Fields: `type`, `seq`, `ts`, `status`.
Example:
```
{"type":"ack","seq":2,"ts":1738291200210,"status":"ok"}
```

### error (either direction)
Fields: `type`, `seq`, `ts`, `code`, `message`.
Example:
```
{"type":"error","seq":3,"ts":1738291200310,"code":"not_controller","message":"Only controller may send commands."}
```

## Control
### control (client -> game)
Fields: `type`, `seq`, `ts`, `action: "claim" | "release"`.
Examples:
```
{"type":"control","seq":10,"ts":1738291200400,"action":"claim"}
{"type":"control","seq":11,"ts":1738291200500,"action":"release"}
```

## Observations
### observation (game -> client)
Fields: `type`, `seq`, `ts`, `playable`, `paused`, `game_over`, `episode_id`, `seed`, `piece_id`, `step_in_piece`, `board`, `board_id`, `active`, `ghost_y`, `next`, `next_queue`, `hold`, `can_hold`, `last_event`, `state_hash`, `score`, `level`, `lines`, `timers`.

Optional fields may be omitted when null/unknown (common early in an episode): `active`, `ghost_y`, `hold`, `last_event`.
`capabilities.features` indicates which optional fields **may** be emitted by this adapter, not that they are always present in every snapshot.


Notes:
- `board_id` increments when the locked board changes (piece lock and/or line clear). It is stable while only the active/ghost/UI changes.
- `ghost_y` is the landing y for the active piece (null if no active).
- `step_in_piece` increments once per fixed step while an active piece exists, including during the line clear pause (`timers.line_clear_ms > 0`).
- `timers.lock_ms` is the grounded lock delay timer: it stays `0` while the active piece can still move down, and increments only while grounded.
Example:
```
{"type":"observation","seq":20,"ts":1738291200600,"playable":true,"paused":false,"game_over":false,"episode_id":0,"seed":1,"piece_id":12,"step_in_piece":0,"board":{"width":10,"height":20,"cells":[[0,0,0,0,0,0,0,0,0,0]]},"board_id":42,"active":{"kind":"t","rotation":"north","x":4,"y":19},"ghost_y":19,"next":"i","next_queue":["i","o","t","s"],"hold":null,"can_hold":true,"last_event":{"locked":true,"lines_cleared":2,"line_clear_score":1200,"tspin":"full","combo":1,"back_to_back":true},"state_hash":"e1bca4d1b673b8c2","score":0,"level":1,"lines":0,"timers":{"drop_ms":1000,"lock_ms":0,"line_clear_ms":0}}
```

## Error Codes (current)
- `handshake_required`: command/control before hello
- `protocol_mismatch`: hello version incompatible
- `not_controller`: non-controller sent command/release
- `controller_active`: controller already assigned
- `invalid_command`: JSON parse/shape errors, unknown message type, or `seq` not strictly increasing
- `invalid_place`: place command could not be mapped/applied
- `hold_unavailable`: hold requested when unavailable
- `snapshot_required`: snapshot required for mapping
- `backpressure`: command queue full

## Recommended Self-Tests
- Hello sequencing:
  - Send `hello` with `seq!=1`.
  - Expect `error.code = "invalid_command"` and the connection to remain unhandshaken (no snapshot request).
- Protocol mismatch:
  - Send `hello` with a different major version (for example `3.0.0` when server is `2.0.0`).
  - Expect `error.code = "protocol_mismatch"` and matching `seq`.
- Backpressure:
  - Set queue limit small (for example `TETRIS_AI_MAX_PENDING=1`).
  - Send two controller `command` messages quickly before the first is drained.
  - Expect one successful `ack` and one `error.code = "backpressure"` (matching each command `seq`).
- Sequencing (out-of-order):
  - Send `hello` with `seq=1`.
  - Then send a `command` with `seq=1` (duplicate / out-of-order).
  - Expect `error.code = "invalid_command"` and matching `seq=1`, and the command MUST NOT be enqueued/applied.

## Defaults
- Default bind: `127.0.0.1:7777` (override with `TETRIS_AI_HOST` / `TETRIS_AI_PORT`).

## Environment Variables
- `TETRIS_AI_HOST`: bind host (default: `127.0.0.1`)
- `TETRIS_AI_PORT`: bind port (default: `7777`)
- `TETRIS_AI_DISABLED`: disable adapter (`1`/`true`)
- `TETRIS_AI_OBS_HZ`: observation frequency in Hz (default: `20`)
- `TETRIS_AI_MAX_PENDING`: max queued controller commands before `backpressure` (default: `10`)
- `TETRIS_AI_LOG_PATH`: append raw adapter traffic to a file (one line per frame; each line is the raw JSON frame)
- `TETRIS_AI_LOG_EVERY_N`: only log every Nth frame (default: 1)
- `TETRIS_AI_LOG_MAX_LINES`: stop logging after N lines (default: unlimited)
