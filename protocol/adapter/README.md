# Tetris AI Adapter Protocol

This directory is the single current protocol package. The current version is
recorded in `VERSION`; protocol history and upgrade notes are recorded in
`CHANGELOG.md`.

## Contents

- `SPEC.md`: normative, implementation-neutral wire and lifecycle contract.
- `schema.json`: machine-readable JSON Schema for protocol messages.
- `profiles/tcp-json-lines.md`: TCP newline-delimited JSON transport profile.
- `conformance/adapter_verify.py`: selected black-box happy-path checks.
- `CHANGELOG.md`: protocol changes relevant to implementers.
- `VERSION`: current protocol version.

## Upgrade policy

Maintain only this latest protocol package. When the protocol changes, update the existing files in place,
bump `VERSION` according to semantic versioning,
and add an entry to `CHANGELOG.md`. Do not create per-version directories.

After publishing an upgrade, notify dependent projects and their agents so they
can review the changelog and align promptly. Dependent projects should record
the protocol version they currently implement; they do not need to copy this
package or pin a release directory.

Canonical source repository:
<https://github.com/lgn21st/tui-tetris>.

## Verification

Start the adapter implementation, then run:

```bash
python3 conformance/adapter_verify.py all --host 127.0.0.1 --port 7777
```

The bundled client exercises selected portable happy paths: readiness, control
claim, restart, and fixed-seed queue determinism. Passing it does not constitute full protocol conformance.
Implementers MUST also cover the normative matrix in `SPEC.md`, including
sequencing, authorization, atomic placement, backpressure, slow-client
isolation, and reconnect behavior. Each project must separately test its
process lifecycle, resource limits, internal queues, logging, and
implementation-specific concurrency behavior in its implementation profile.
