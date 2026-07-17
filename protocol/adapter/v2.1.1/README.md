# Tetris AI Adapter Protocol 2.1.1

This directory is the self-contained release bundle for adapter protocol
version 2.1.1.

Canonical source repository:
<https://github.com/lgn21st/tui-tetris>. A commit SHA in this document refers
to that Git repository unless a distributor records an equivalent provenance.

## Contents

- `SPEC.md`: normative, implementation-neutral wire and lifecycle contract.
- `schema.json`: machine-readable JSON Schema for protocol messages.
- `profiles/tcp-json-lines.md`: TCP newline-delimited JSON transport profile.
- `conformance/adapter_verify.py`: language-neutral black-box verification client.
- `CHANGELOG.md`: protocol changes relevant to implementers.
- `VERSION`: exact protocol version.

## Pinning

Consumers MUST pin both this version directory and the repository commit SHA.
Do not track a mutable branch when claiming protocol conformance. A project
should record the version, commit SHA, selected transport profile, and any local
deviations in its implementation profile.

Once published, treat this as an immutable release directory. Corrections that
change normative behavior require a new protocol version and directory. Purely
editorial corrections should still be traceable by commit SHA.

## Verification

Start the adapter implementation, then run:

```bash
python3 conformance/adapter_verify.py all --host 127.0.0.1 --port 7777
```

The bundled client exercises selected portable happy paths: readiness, control
claim, restart, and fixed-seed queue determinism. Passing it does not constitute full protocol conformance.
Implementers MUST also cover the normative matrix in
`SPEC.md`, including sequencing, authorization, atomic placement, backpressure,
slow-client isolation, and reconnect behavior. Each project must separately
test its process lifecycle, resource limits, internal queues, logging, and
implementation-specific concurrency behavior.
