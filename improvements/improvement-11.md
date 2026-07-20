# Improvement 11 — Source-coordinate styling recovery

Status: done

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

## Problem

Bold, italic, heading, and CSS-class emphasis are recovered by searching the
rendered text twice. Wrapping, justification, indentation, Unicode, and
synthetic rows can therefore move or drop styles.

## Goal

Store semantic styles as chapter-local normalized source ranges and derive the
existing row-based `InlineStyle` values through `SourceMap`.

## Requirements

- Render a copy of the HTML with `strong`, `b`, `em`, and `i` neutralized so
  html2text does not insert emphasis markers; preserve literal asterisks.
- Walk DOM text in document order and locate styled semantic tags and configured
  CSS classes in normalized source coordinates.
- Preserve nested bold/italic attributes, headings, Unicode character units,
  repeated-occurrence identity, and chapter starting-row offsets.
- Project each canonical range over every overlapping non-synthetic source-map
  row, including wrapped, hyphenated, justified, indented, and image-padded
  layouts.
- Retire the rendered-text matcher and marker-stripping helpers.

## Verification

Run `cargo fmt --all`, `cargo clippy --all-targets`, and `cargo test`. Mark this
file done and Phase 7 item 2 complete only after all checks pass.
