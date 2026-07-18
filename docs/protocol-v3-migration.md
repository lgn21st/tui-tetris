# Protocol v3 Client Migration

Protocol `3.0.0` intentionally rejects `2.1.1` clients. A dependent client must
make these changes as one atomic upgrade:

1. Send `protocol_version: "3.0.0"` in hello and require major version 3 in the
   welcome response.
2. Replace nullable observation `last_event` handling with the required bounded
   `events` array. Process its entries in order; an empty array means that the
   represented transition emitted no event.
3. Read the required observation `logical_step` as the authoritative causal
   step. Observation `seq` remains an independent delivery sequence and may
   contain gaps.
4. Require `correlation_seq` on every ack. For a successful game command, also
   require `applied_step` and `state_hash`; control acks omit those two fields.
5. Regenerate or update wire models from `protocol/adapter/schema.json`, then
   run the client's deterministic restart and closed-loop suites against the
   bundled conformance client behavior.

## Notification prompt for dependent project agents

> Upgrade this client from Tetris AI Adapter Protocol 2.1.1 to 3.0.0. Follow
> `protocol/adapter/SPEC.md` and `schema.json` as authoritative: replace
> `last_event` with ordered `events[]`, consume observation `logical_step`, and
> validate ack `correlation_seq` plus command-only `applied_step` and
> `state_hash`. Reject v2/v3 mixing, update fixtures and models, and run the
> client's handshake, deterministic restart, ack-correlation, and closed-loop
> tests before changing its advertised protocol version.
