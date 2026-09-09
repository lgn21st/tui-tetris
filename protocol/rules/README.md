# Tetris Ruleset (Guideline DS/Friends subset)

This directory is the single current **portable rules** package. The current
version is recorded in `VERSION`. It is independent of the AI adapter protocol
in `protocol/adapter/`.

This ruleset freezes the Tetris Guideline behavior documented for
**Tetris DS / Tetris Friends** era games on tetris.wiki. It is not a TTC
certification suite and it does not cover every Guideline-era variant
(Tetris Effect Perfect Clear, tetris.com spawn rows, 20G marathon).

## Contents

- `SPEC.md`: normative geometry, 7-bag, lock delay, scoring, and T-Spin/B2B/combo
- `constants.json`: machine-readable tables (kicks are wiki Y-up)
- `CHANGELOG.md`: ruleset changes for implementers
- `VERSION`: current ruleset version (`guideline-ds-1.0.0` in replay metadata)

## Upgrade policy

Maintain only this latest package. When rules change, update these files in
place, bump `VERSION` according to semantic versioning, and append
`CHANGELOG.md`. Do not create per-version directories.

After an upgrade, notify dependent projects. Implementations record the
ruleset version in replay metadata and must not copy local timing
(DAS/ARR, ticks, line-clear pause, gravity) into this package.

Canonical source repository: https://github.com/lgn21st/tui-tetris

## Coordinate conversion

`SPEC.md` and `constants.json` use wiki convention: **+x right, +y up**.
Playfields that increase y downward MUST negate kick `dy` (and only `dy`).
