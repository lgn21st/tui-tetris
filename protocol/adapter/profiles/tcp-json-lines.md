# TCP JSON-Lines Profile 1

This profile binds Tetris AI Adapter Protocol 2.1.1 to a localhost TCP stream.
The protocol specification remains authoritative for message and lifecycle
semantics.

## Endpoint and framing

- Default address: `127.0.0.1:7777`.
- Each frame MUST be one UTF-8 JSON object followed by `\n`.
- A receiver MUST ignore or reject an empty line and MUST NOT interpret it as a
  valid protocol message.
- A receiver MUST accept frames containing up to 65,536 payload bytes, excluding
  the newline terminator.
- A receiver MUST bound memory while reading. It MUST reject or close a
  connection whose unterminated frame exceeds 65,536 payload bytes.
- Invalid UTF-8 MUST NOT be decoded lossily into a valid command.

## Delivery and resource behavior

- Implementations MUST use bounded inbound and outbound buffering.
- Backpressure MUST NOT block the authoritative game loop indefinitely.
- Welcome, ack, error, and targeted non-observation responses MUST NOT be
  silently dropped.
- A slow or dead connection MUST NOT delay unrelated clients.
- Observation delivery MAY coalesce older pending full snapshots. Clients MUST
  use observation `seq` values to detect gaps and treat the newest snapshot as
  authoritative.
- Queue capacities, synchronization primitives, writer timeouts, and logging
  mechanisms are implementation-profile decisions.

## Common process configuration

Executable adapters SHOULD recognize this portable configuration subset:

| Variable | Default | Meaning |
| --- | --- | --- |
| `TETRIS_AI_HOST` | `127.0.0.1` | TCP bind host |
| `TETRIS_AI_PORT` | `7777` | TCP bind port |
| `TETRIS_AI_DISABLED` | unset | `1` or `true` disables the adapter |

Additional environment variables belong in the implementation profile and MUST
NOT be treated as cross-project protocol requirements.
