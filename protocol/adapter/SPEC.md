# Tetris AI Adapter Protocol 2.1.1

This document is the normative, implementation-neutral contract between a
Tetris game adapter and an AI client. The key words MUST, MUST NOT, SHOULD,
SHOULD NOT, and MAY are normative.

The standalone message schema is `schema.json`. Transport behavior is defined
by a selected profile such as `profiles/tcp-json-lines.md`.

## 1. Versioning and compatibility

- Every hello and welcome contains `protocol_version` as semantic version text.
- Implementations of this release report `2.1.1` in welcome messages.
- Version 2.1.1 is backward compatible with valid `2.x` clients.
- A server MUST reject an incompatible major version with `protocol_mismatch`.
- A server MUST reject malformed semantic versions.
- Patch releases preserve message shapes and error-code compatibility.

## 2. Common message envelope

Every message contains:

- `type`: message type string.
- `seq`: unsigned sequence or correlation number.
- `ts`: Unix timestamp in milliseconds.

The message types are hello, welcome, command, control, observation, ack, and
error.

## 3. Handshake

The first valid client message MUST be hello.

Hello requires `type`, `seq`, `ts`, `client`, `protocol_version`, `formats`, and
`requested`.

- `hello.seq` MUST equal `1`.
- `formats` MUST contain `json`.
- `requested.command_mode` declares a preference; welcome capabilities remain
  authoritative.
- `requested.role` MAY be `auto`, `controller`, or `observer`.
- Commands or control messages received before a valid hello return
  `handshake_required`.

Example:

```json
{"type":"hello","seq":1,"ts":1738291200000,"client":{"name":"tetris-ai","version":"0.1.0"},"protocol_version":"2.1.1","formats":["json"],"requested":{"stream_observations":true,"command_mode":"place","role":"auto"}}
```

Welcome requires `type`, `seq`, `ts`, `protocol_version`, `game_id`,
`capabilities`, `client_id`, `role`, and `controller_id`.

- `welcome.seq` echoes `hello.seq`.
- `client_id` is stable for the connection and unique among concurrent clients.
- `role` reports the role assigned at handshake time.
- `controller_id` is the active controller id or null.
- `capabilities.features` is the union of `features_always` and
  `features_optional`.

Example:

```json
{"type":"welcome","seq":1,"ts":1738291200100,"protocol_version":"2.1.1","client_id":1,"role":"controller","controller_id":1,"game_id":"example-game","capabilities":{"formats":["json"],"command_modes":["action","place"],"features":["hold","next","next_queue","can_hold","ghost_y","board_id","last_event","state_hash","score","timers"],"features_always":["next","next_queue","can_hold","board_id","state_hash","score","timers"],"features_optional":["hold","ghost_y","last_event"],"control_policy":{"auto_promote_on_disconnect":true,"promotion_order":"lowest_client_id"}}}
```

## 4. Sequencing and correlation

- After welcome, every command and control `seq` MUST be strictly greater than
  the previous client sequence on that connection.
- Duplicate or decreasing sequences return `invalid_command` and MUST NOT be
  enqueued or applied.
- Ack and error `seq` echo the triggering client sequence when available.
- An unparseable frame MAY produce `error.seq = 0`.
- Observation sequences form an independent monotonically increasing stream.
- Clients MUST NOT compare observation sequences with ack/error sequences.
- Observation sequence gaps are valid.
- A command rejected with `backpressure` was not enqueued or applied. A retry
  uses a new, larger sequence.

## 5. Roles and control

- There is at most one controller.
- Only the controller may send command messages.
- Observer commands return `not_controller`.
- A client requesting role `observer` MUST NOT become controller as a side
  effect of hello or automatic disconnect promotion.
- `control(action="claim")` is idempotent for the active controller.
- Claim assigns an unowned controller role or returns `controller_active` when a
  different controller exists.
- Only the controller may release; other clients receive `not_controller`.
- Successful release returns ack and clears the assignment.
- Disconnect cleanup MUST remove stale controller assignments.
- An implementation MAY promote an eligible client after controller disconnect,
  but its stable policy MUST be exposed in
  `welcome.capabilities.control_policy`.
- This release has no asynchronous role-change message. Clients discover their
  effective authorization through later ack/error responses.

## 6. Commands

### 6.1 Action mode

```json
{"type":"command","seq":7,"ts":1730000001300,"mode":"action","actions":["rotateCw","moveLeft","hardDrop"]}
```

- `actions` MAY be empty and contains at most 32 actions.
- Standard actions are moveLeft, moveRight, softDrop, hardDrop, rotateCw,
  rotateCcw, hold, pause, and restart.
- If restart parameters are present, actions MUST contain `restart`.
- `restart.seed` is an unsigned 32-bit integer.

### 6.2 Place mode

```json
{"type":"command","seq":8,"ts":1730000001300,"mode":"place","place":{"x":3,"rotation":"east","useHold":false}}
```

- `place.x` is the tetromino origin, not necessarily the leftmost occupied cell.
- Invalid or unreachable placements return `invalid_place`.
- Unavailable requested hold returns `hold_unavailable`.
- Place application MUST be atomic. Failure leaves board, active piece,
  hold/queue state, timers, score, and lifecycle state unchanged.

### 6.3 Application ordering

The authoritative game-side order is:

1. drain and apply accepted commands;
2. advance game rules;
3. emit observations.

An adapter sends ack only after the authoritative game loop applies the command.
Successful enqueue alone is insufficient.

## 7. Observations

Every observation is a full snapshot, not a delta.

Required fields:

- `type`, `seq`, `ts`;
- `playable`, `paused`, `game_over`;
- `episode_id`, `seed`, `piece_id`, `step_in_piece`;
- `board`, `board_id`;
- `next`, `next_queue`, `can_hold`;
- `state_hash`;
- `score`, `level`, `lines`;
- `timers`.

Data invariants:

- Board width is 10 and height is 20.
- Cell value 0 is empty; values 1 through 7 map to I, O, T, S, Z, J, L.
- `next_queue` contains exactly five pieces.
- `next == next_queue[0]`.
- `active` is present when `playable` is true and MAY be absent otherwise.
- `ghost_y`, `hold`, and `last_event` are optional.
- Clients SHOULD accept optional fields as either omitted or explicit null.
- `board_id` changes only when locked board cells change.
- `state_hash` is an opaque 16-character lowercase hexadecimal digest.
- Hash equality is meaningful only within the same implementation and ruleset
  version.

Example:

```json
{"type":"observation","seq":42,"ts":1730000001200,"playable":true,"paused":false,"game_over":false,"episode_id":0,"seed":1,"piece_id":12,"step_in_piece":0,"board":{"width":10,"height":20,"cells":[[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0]]},"board_id":123,"active":{"kind":"t","rotation":"north","x":4,"y":0},"ghost_y":17,"next":"i","next_queue":["i","o","t","s","z"],"can_hold":true,"state_hash":"e1bca4d1b673b8c2","score":1200,"level":2,"lines":17,"timers":{"drop_ms":320,"lock_ms":120,"line_clear_ms":0}}
```

## 8. Lifecycle and determinism

- `playable` describes game lifecycle, not client authorization.
- `playable=true` means the game is neither paused nor game-over and can advance
  for an authorized controller.
- Observers MAY receive playable snapshots without command authority.
- Pause behavior and ignored gameplay actions MUST be deterministic.
- Restart returns ack or a typed error.
- A successful restart produces a fresh episode and playable observation within
  a bounded implementation-defined interval; two seconds is recommended.
- A supplied restart seed derives the implementation's complete gameplay RNG
  stream.
- Equal implementation, ruleset, seed, initial state, and command sequence MUST
  produce identical gameplay trajectories.
- Cross-implementation state hashes and piece sequences need not match.

## 9. Delivery and backpressure

- Streaming hello requests an immediate full snapshot.
- Implementations SHOULD emit immediate snapshots for piece spawn/lock, pause
  changes, and game-over changes.
- Observation delivery is latest-state oriented and MAY skip superseded full
  snapshots.
- Welcome, ack, error, and targeted responses MUST NOT be silently dropped.
- Resource exhaustion MUST remain isolated from unrelated clients and the
  authoritative game loop.
- A full inbound command queue returns `backpressure` without applying the
  command and SHOULD include positive `retry_after_ms`.

## 10. Errors

Required error codes:

- `handshake_required`
- `protocol_mismatch`
- `not_controller`
- `controller_active`
- `invalid_command`
- `invalid_place`
- `hold_unavailable`
- `snapshot_required`
- `backpressure`

## 11. Conformance

Conformance requires evidence for:

- handshake, version, framing, and sequencing;
- deterministic controller/observer lifecycle;
- command authorization and post-application acknowledgement;
- atomic place failures;
- complete schema-valid observations;
- deterministic seeded restart;
- bounded backpressure and slow-client isolation;
- reconnect and closed-loop stability.

The bundled black-box client covers the portable happy paths. Implementations
must add local tests for error paths, resource bounds, and process cleanup.
