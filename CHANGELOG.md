# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added
- AI Adapter with TCP protocol (100% compatible with swiftui-tetris)
  - Controller/Observer pattern for multiplayer AI
  - Action and Place command modes
  - Observation streaming with full game state
  - Protocol version 2.0.0 compliance
- Professional DAS/ARR input handling
  - Delayed Auto Shift: 167ms
  - Auto Repeat Rate: 33ms
  - Ghost key elimination for rapid alternation
- Aspect ratio correction (2:1 character ratio for square blocks)
- Terminal-first renderer (framebuffer + diff/dirty-cell flush)
- Engine-facing input module (key mapping + DAS/ARR)
- Comprehensive rustdoc documentation with doc-test examples
- Input test utility (`cargo run --bin input-test`)

### Fixed
- Locked pieces not displaying after landing
- Line clear row shifting algorithm (now clears from top to bottom)
- Terminal compatibility for Ghostty (no key release events)
- "Device not configured" error by removing TTY check

### Changed
- Replaced ratatui UI with a custom crossterm + framebuffer renderer
- Moved terminal input into `src/input` (no UI-framework coupling)
- Switched to diff/dirty-cell flushing for terminal performance

### Technical
- 137 tests passing (including 8 doc tests)
- Zero compiler warnings
- Complete API documentation
- Modular adapter architecture ready for extensions

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
