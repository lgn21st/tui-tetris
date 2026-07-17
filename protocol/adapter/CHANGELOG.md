# Adapter Protocol Changelog

## 2.1.1

- Published the protocol as a single current package at `protocol/adapter/`;
  future upgrades update this path in place and notify dependent projects.
- Clarified strict semantic-version validation while preserving compatibility
  with valid `2.x` clients.
- Defined bounded TCP framing in the TCP JSON-lines profile.
- Clarified that observation sequence gaps are valid because observations are
  full, latest-state snapshots.
- Required correlated welcome/ack/error delivery to avoid silent response loss.
- Clarified lifecycle playability, place atomicity, and deterministic restart
  behavior without changing message shapes or error codes.

## 2.1.0

- Added deterministic `client_id`, `role`, and `controller_id` welcome fields.
- Added explicit controller policy capabilities.
- Partitioned always-present and optional observation capabilities.
- Added optional backpressure retry hints.
