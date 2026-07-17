# Tetris AI Adapter Protocol Releases

Each `v<semver>/` directory is a self-contained, versioned protocol release.
Consumers pin both the directory version and a repository commit SHA.

Current release: `v2.1.1/`.

Release directories contain the normative specification, JSON Schema, transport
profiles, changelog, and black-box conformance client. Project-specific runtime
and architecture documentation must remain outside these directories.

Published release directories are immutable. A normative correction or behavior
change requires a new semantic version and release directory.
