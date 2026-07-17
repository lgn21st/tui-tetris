# tui-tetris Adapter Implementation Profile

This profile documents tui-tetris behavior for the shared protocol release at
`protocol/adapter/v2.1.1/SPEC.md`. It is not a portable implementation mandate.

## Protocol and transport

- Protocol version: `2.1.1`.
- TCP profile: `protocol/adapter/v2.1.1/profiles/tcp-json-lines.md`.
- Default endpoint: `127.0.0.1:7777`.
- Maximum inbound payload: 65,536 bytes, excluding newline.
- Invalid UTF-8 and oversized frames close the connection.

## Controller policy

- The first handshaken `auto` or `controller` client becomes controller when
  none exists.
- A requested `observer` remains observer-locked for automatic promotion.
- Explicit release leaves the controller unassigned until claim.
- Controller disconnect promotes the eligible connected client with the lowest
  client id.
- Welcome reports `auto_promote_on_disconnect=true` and
  `promotion_order=lowest_client_id`.

## Command application

- The interactive and headless runners share adapter-owned command draining.
- The fixed-step order is command application, game tick, then observation.
- Ack is emitted only after authoritative command application.
- Place commands execute directly against core state and roll back atomically on
  every error; this implementation does not emit `snapshot_required`.

## Observation scheduling and delivery

- Default frequency: 20 Hz, configurable from 1 through 60 Hz.
- Scheduling uses a fixed-step phase accumulator so non-divisor frequencies do
  not accumulate integer-period drift.
- Piece, lock, pause, and game-over transitions request immediate snapshots.
- No periodic observation is built when no streaming subscriber exists.
- Full observations are shared for fanout and each client retains only the most
  recent pending observation.
- reliable queue capacity: `32` messages per client.
- Reliable overflow closes only the slow client; correlated responses are not
  silently dropped.
- Adapter status is latest-only rather than an unbounded history.

## Runtime configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `TETRIS_AI_HOST` | `127.0.0.1` | Bind host |
| `TETRIS_AI_PORT` | `7777` | Bind port; `0` selects an ephemeral test port |
| `TETRIS_AI_DISABLED` | unset | `1` or `true` disables the adapter |
| `TETRIS_AI_MAX_PENDING` | `10` | Inbound command capacity |
| `TETRIS_AI_OBS_HZ` | `20` | Observation frequency, clamped to 1..60 |
| `TETRIS_AI_LOG_PATH` | unset | Optional newline-delimited wire log |
| `TETRIS_AI_LOG_EVERY_N` | `1` | Log sampling interval |
| `TETRIS_AI_LOG_MAX_LINES` | unlimited | Optional persisted-line limit |

## Logging and startup

- wire-log queue capacity: `1,024` best-effort records.
- Log storage latency and failure never participate in protocol ordering.
- Dropped diagnostic log records are allowed; the wire log is not an audit log.
- The async server performs the single authoritative bind and reports the actual
  address or bind error to the synchronous caller.
- Socket or disk I/O is not awaited while controller/client locks are held.
- Writer shutdown is bounded so dead peers do not retain tasks indefinitely.

## Local verification

The stable local entry point delegates to the versioned conformance client:

```bash
python3 scripts/adapter_verify.py all
```

Repository validation:

```bash
cargo test --lib adapter
cargo test --test adapter_acceptance_test
cargo test --test adapter_e2e_test
cargo test --test adapter_closed_loop_test
cargo test --test adapter_docs_test
cargo test --test adapter_observation_no_alloc_gate_test
```
