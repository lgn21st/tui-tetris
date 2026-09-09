# Adapter Protocol Changelog

## 3.0.0

Current wire contract. Older protocol versions are not maintained.

- Observations carry a bounded ordered `events` array and `logical_step`
- Every ack has `correlation_seq`; successful game-command acks also have
  `applied_step` and `state_hash`
- Hello/welcome `protocol_version` is major 3; v2 clients (including those
  that used nullable `last_event`) are rejected
