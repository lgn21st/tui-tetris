# AI Adapter Documentation

tui-tetris implements Tetris AI Adapter Protocol 3.2.0.

## Shared current protocol

- `protocol/adapter/SPEC.md`
- `protocol/adapter/schema.json`
- `protocol/adapter/profiles/tcp-json-lines.md`
- `protocol/adapter/conformance/adapter_verify.py`
- `protocol/adapter/VERSION`
- `protocol/adapter/CHANGELOG.md`

Only the latest protocol is maintained. Dependent projects should align to the
changelog and must not copy tui-tetris implementation details into the shared
contract.

## tui-tetris implementation

Lifecycle, queue capacities, scheduling, environment variables, logging,
startup, and local validation: `docs/adapter-tui-tetris.md`.
