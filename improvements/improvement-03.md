# Improvement 03 — Fuzzy filter in the Help window

Status: todo

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

## Problem

Phase 1 added `/` fuzzy filtering to the TOC, library, bookmarks, and
highlights windows (shared helper in `src/ui/windows/mod.rs`, `nucleo-matcher`),
but the Help window (`src/ui/windows/help.rs`) — the longest list in the app —
can only be scrolled. Finding one keybinding means paging through everything.

## Goal

`/` inside the Help window fuzzy-filters help lines the same way the other
list windows do.

## Requirements

1. Reuse the existing shared fuzzy-filter helper and the same interaction
   grammar as the other windows: `/` opens the filter prompt, typing narrows
   live, `Esc` clears the filter, `q`/`Esc` with no active filter closes the
   window as before.
2. Filter over the keybinding lines. Keep a section header visible when any of
   its lines match, so results stay readable; drop sections with no matches.
   (If the current help content is a flat `&[&str]`, it already distinguishes
   headers via `is_section_header` — reuse that.)
3. Scrolling and `max_scroll_offset` must operate on the filtered content;
   entering/clearing the filter resets scroll to the top.
4. There is no Enter-to-act here (help lines are not actionable); Enter can
   just close the prompt and keep the filter applied.
5. Update the Help window's own text and README.md ("Windows & Tools" and the
   fuzzy-filter mention) in the same commit, per the project keybinding rule.

## Tests

- Unit tests in `src/ui/windows/help.rs` for the filtering logic (matching
  lines kept, empty sections dropped, scroll bounds recomputed).
- A TUI snapshot test in `src/ui/reader/snapshot_tests.rs`: open help, press
  `/`, type a query (e.g. `bookmark`), snapshot the filtered window.

## Out of scope

Making help lines actionable (jump-to-setting etc.), changing help content
otherwise, filter history.
