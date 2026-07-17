# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added
- AI Adapter with a versioned protocol release bundle (`protocol/adapter/v2.1.1/`)
  - Controller/Observer pattern for multiplayer AI
  - Action and Place command modes
  - Observation streaming with full game state
  - Protocol version 2.1.1 compliance
- Professional DAS/ARR input handling
  - Delayed Auto Shift: 150ms
  - Auto Repeat Rate: 50ms
  - Ghost key elimination for rapid alternation
- Aspect ratio correction (2:1 character ratio for square blocks)
- Terminal-first renderer (framebuffer + diff/dirty-cell flush)
- Injectable renderer output backend with regression benchmarks for the full render pipeline
- Engine-facing input module (key mapping + DAS/ARR)
- Comprehensive rustdoc documentation with doc-test examples
- Input test utility (`cargo run --bin input-test`)

### Fixed
- Locked pieces not displaying after landing
- Line clear row shifting algorithm (now clears from top to bottom)
- T-Spin classification now occurs before cleared rows shift corner occupancy
- Failed place commands no longer retain partial movement, rotation, or hold state
- Adapter protocol 2.1.1 incorporates backward-compatible framing, validation,
  cadence, backpressure, and startup hardening
- Observation cadence preserves the configured long-run frequency
- Fixed-step runners retain and process elapsed backlog after temporary stalls
- Drop and scoring helpers saturate instead of overflowing at numeric limits
- Zero ARR configuration is clamped to a safe one-millisecond minimum
- Terminal compatibility for Ghostty (no key release events)
- "Device not configured" error by removing TTY check

### Changed
- Replaced ratatui UI with a custom crossterm + framebuffer renderer
- Moved terminal input into `src/input` (no UI-framework coupling)
- Switched to diff/dirty-cell flushing for terminal performance

### Technical
- Allocation gates for core, input, adapter observation, rendering, and the no-I/O end-to-end path
- Criterion regression gates for game logic, adapter serialization, and renderer pipelines
- Clippy clean with warnings treated as errors
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

---

**Enjoy the game!** 🎮
