# AI Adapter Documentation

tui-tetris implements Tetris AI Adapter Protocol 3.0.0.

## Shared current protocol

- Protocol specification: `protocol/adapter/SPEC.md`
- JSON Schema: `protocol/adapter/schema.json`
- TCP profile: `protocol/adapter/profiles/tcp-json-lines.md`
- Conformance client:
  `protocol/adapter/conformance/adapter_verify.py`
- Current version: `protocol/adapter/VERSION`
- Protocol changelog: `protocol/adapter/CHANGELOG.md`
- v2 to v3 client migration: `docs/protocol-v3-migration.md`

Only the latest protocol is maintained. When its version changes, dependent
projects are notified to review the changelog and align their implementations.
They should not copy tui-tetris implementation details into the shared contract.

## tui-tetris implementation

Project-specific lifecycle choices, queue capacities, scheduling, environment
variables, logging, startup behavior, and local validation commands are in
`docs/adapter-tui-tetris.md`.

The shared protocol package is the compatibility contract. The local profile documents
how this repository satisfies it.
