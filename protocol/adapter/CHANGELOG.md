# Adapter Protocol Changelog

## 3.2.0

- Welcome carries `ruleset_id`: the implementation-defined stable identifier of
  the authoritative ruleset and its version, for example `guideline-ds-1.1.0`
- `protocol_version` covers the wire and lifecycle contract only. A ruleset
  change may leave it unchanged, so a client that pins expected gameplay
  behavior must compare `ruleset_id`
- Purely additive field: 3.1 clients that ignore unknown fields stay compatible

## 3.1.0

- Observations include current HUD chain state: `combo` and `back_to_back`
- Welcome `features` / `features_always` advertise `combo` and `back_to_back`
- Missing `combo` deserializes as `-1`; missing `back_to_back` deserializes as `false`

## 3.0.0

Current wire contract. Older protocol versions are not maintained.

- Observations carry a bounded ordered `events` array and `logical_step`
- Every ack has `correlation_seq`; successful game-command acks also have
  `applied_step` and `state_hash`
- Hello/welcome `protocol_version` is major 3; v2 clients (including those
  that used nullable `last_event`) are rejected
