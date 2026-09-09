# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Fixed
- Unhandshaken TCP sockets are no longer promotion candidates for controller
- O-piece rotation/place now advances SRS rotation with a `(0,0)` kick
- Core retains up to four ordered lock events per step instead of overwriting
- Welcome `features_always` includes `events` and `logical_step`; `features` is a unique union
- Gravity uses `DROP_INTERVALS` / level-9 floor of 120ms; dead 100ms constant removed
- Repeat-release timeout min/max environment variables apply independently
- Clippy `manual_slice_fill` on board clear
- Observation cadence is skipped entirely when no streaming subscriber exists
- `encode_diff_into` / `encode_full_into` write ANSI CSI without allocating
- State hash includes combo, back-to-back, and lock-reset count (`tui-guideline-2026.2`)

### Changed
- Local validation commands are `cargo test --workspace` and `cargo clippy --workspace`
- Adapter internals import `tetris_adapter_protocol` directly instead of reexporting it
- Line-clear pause is documented as gravity-only; the next piece is already spawned
- Replay ruleset version is `tui-guideline-2026.2`

### Added
- `.gitignore`, MIT `LICENSE`, and `rust-version = "1.85"`
- Lock-reset-limit freeze coverage and independent repeat-timeout bounds tests
- TCP coverage for unhandshaken promotion and reliable-output overflow isolation
- Replay TTR2 with ruleset metadata, complete transition hashes,
  record/verify/inspect CLI, and minimal failing-prefix diagnostics
- Source-owning `tetris-core`, `tetris-session`, `tetris-adapter-protocol`,
  `tetris-adapter`, and `tetris-terminal` workspace packages
- TCP stress gates for disconnect storms, slow-client isolation, and 32-observer fanout
- Unified deterministic `StepInput → Transition` session, coherent
  `SnapshotStore`, and reusable fixed-step backlog clock
- Protocol v3 causal observations and applied-state command acknowledgements
- Immutable terminal `GameViewModel`, platform-neutral `InputCommand`, finite
  headless batch mode, and diagnostic CLI
- Architecture boundary tests for core dependencies, composition-root mutation,
  and bounded production adapter outbound delivery
- AI Adapter with a single current protocol package (`protocol/adapter/`)
  - Controller/Observer pattern for multiplayer AI
  - Action and Place command modes
  - Observation streaming with full game state
  - Protocol version 3.0.0 compliance
- Professional DAS/ARR input handling
  - Delayed Auto Shift: 150ms
  - Auto Repeat Rate: 50ms
  - Ghost key elimination for rapid alternation
- Aspect ratio correction (2:1 character ratio for square blocks)
- Terminal-first renderer (framebuffer + diff/dirty-cell flush)
- Injectable renderer output backend with regression benchmarks for the full render pipeline
- Engine-facing input module (key mapping + DAS/ARR)
- Comprehensive rustdoc documentation with doc-test examples
- Input test example (`cargo run --example input-test`)

### Fixed
- Standalone production builds now enable the Tokio features used by Adapter
  selection and wire logging
- Local key presses now apply at the same fixed-step boundary as AI and DAS/ARR actions
- Streaming hello snapshot requests wait for bounded queue capacity instead of
  being silently lost under backpressure
- Locked pieces not displaying after landing
- Line clear row shifting algorithm (now clears from top to bottom)
- T-Spin classification now occurs before cleared rows shift corner occupancy
- Failed place commands no longer retain partial movement, rotation, or hold state
- Adapter framing, validation, cadence, backpressure, and startup behavior are
  bounded and covered under protocol 3.0.0
- Observation cadence preserves the configured long-run frequency
- Fixed-step runners retain and process elapsed backlog after temporary stalls
- Drop and scoring helpers saturate instead of overflowing at numeric limits
- Zero ARR configuration is clamped to a safe one-millisecond minimum
- Terminal compatibility for Ghostty (no key release events)
- "Device not configured" error by removing TTY check

### Changed
- Migrated the root package and all workspace crates to Rust Edition 2024 with
  Cargo dependency resolver 3
- Centralized shared dependency versions, removed unused root dependencies,
  and replaced cross-crate convenience reexports with direct owner-crate imports
- Consolidated adapter TCP test setup, removed redundant smoke/E2E coverage,
  and replaced disconnect sleeps with observable status synchronization
- Acceptance and closed-loop harnesses execute commands through the production
  `SessionProtocolDriver` and `SessionRuntime`
- Global adapter delivery now carries only latest-only Arc observations;
  correlated reliable replies go directly to their client mailbox
- Replaced ratatui UI with a custom crossterm + framebuffer renderer
- Moved terminal input and rendering into `tetris-terminal` (no app coupling)
- Switched to diff/dirty-cell flushing for terminal performance

### Technical
- Single broker state owns the client registry and authoritative controller id
- Post-application ack/error and targeted snapshots use direct per-client responders
- Production adapter broadcast delivery is a single latest-only observation slot
- Adapter state hashes use canonical field encodings rather than Rust `Hash`
- Allocation gates for core, input, adapter observation, rendering, and the no-I/O end-to-end path
- Criterion regression gates for game logic, adapter serialization, and renderer pipelines
- Clippy clean with warnings treated as errors (`cargo clippy --workspace`)
- Shared deterministic adapter command draining and observation scheduling
- Incrementally bounded 64 KiB adapter input framing
- Per-client bounded reliable output queues with latest-observation coalescing
- Bounded best-effort wire logging and latest-only adapter status delivery
- Authoritative adapter bind result propagation without a preflight-bind race

## [0.1.0] - 2025-02-02

### Added
- Initial TUI Tetris implementation
- Core game logic with SRS rotation system
- 7-bag random generator
- T-Spin detection (mini/full)
- Classic + Modern scoring system
- Ghost piece preview
- Hold functionality
- 114 unit tests
- Architecture documentation
- Performance benchmarks
