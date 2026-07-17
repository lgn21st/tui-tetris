# AI Adapter Documentation

tui-tetris implements Tetris AI Adapter Protocol 2.1.1.

## Shared, versioned protocol release

- Protocol specification: `protocol/adapter/v2.1.1/SPEC.md`
- JSON Schema: `protocol/adapter/v2.1.1/schema.json`
- TCP profile: `protocol/adapter/v2.1.1/profiles/tcp-json-lines.md`
- Conformance client:
  `protocol/adapter/v2.1.1/conformance/adapter_verify.py`
- Protocol changelog: `protocol/adapter/v2.1.1/CHANGELOG.md`

External projects should pin the release directory and repository commit SHA.
They should not copy tui-tetris implementation details into the shared contract.

## tui-tetris implementation

Project-specific lifecycle choices, queue capacities, scheduling, environment
variables, logging, startup behavior, and local validation commands are in
`docs/adapter-tui-tetris.md`.

The shared release is the compatibility contract. The local profile documents
how this repository satisfies it.
