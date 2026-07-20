# Improvement 06 — Styling lookahead must survive justified whitespace runs

Status: todo

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

Context: ROADMAP.md Phase 7 item 2 (short-term fix). Independent of
improvements 07–10.

## Problem

`match_sequence` in `src/parser.rs` (around line 1349) recovers bold/italic
coordinates by locating each whitespace-separated token of a formatted run in
the rendered lines. It looks for the next token inside a fixed window:

- same-line continuation: `lookahead_limit = 20` bytes at `src/parser.rs:1364`
- next-line continuation: `lookahead_limit = 20` bytes at `src/parser.rs:1424`

With justification enabled, a sparsely justified line (e.g. two words pushed
to opposite margins of an 80-column line) separates adjacent tokens by far
more than 20 bytes of spaces. `search_slice.find(token)` then fails, the
function returns `None`, and the whole run silently loses its bold/italic
styling. First-line indents (two leading columns in `indented` paragraph
style) can similarly eat into the next-line window.

## Goal

A formatting run must keep its coordinates no matter how much *whitespace*
layout inserted between its tokens. The 20-byte window should limit how much
*non-whitespace* garbage we scan past, not how many spaces.

## Requirements

1. In both lookahead sites, skip the leading whitespace run first, then apply
   the existing 20-byte window to what follows. Concretely: advance a cursor
   over `char::is_whitespace` from `current_pos` (same-line case) / from
   column 0 (next-line case), take `safe_slice(line, cursor, 20)`, and search
   there. Gap validation must still run over the *entire* gap (whitespace run
   included) via `is_valid_gap` (`src/parser.rs:1457`), so the accepted
   gap characters do not change — only the window placement does.
2. Keep `safe_slice`'s UTF-8 boundary care; the new cursor arithmetic must
   also never slice inside a multi-byte character (whitespace can be
   non-ASCII, e.g. NBSP — decide explicitly whether it counts as skippable
   whitespace and document the choice in a comment).
3. The hyphenation fallback path in the same function (line-tail
   `" <prefix>-"` handling) must behave exactly as before for inputs that
   previously matched.
4. No public API changes; this is internal to `src/parser.rs`.

## Tests

Codex writes the tests. Cover at least:

- A two-token bold run on one line separated by ~40 spaces (simulating heavy
  justification) keeps both segments' coordinates.
- A run continuing on the next line where the next line starts with a wide
  indent (more than 20 spaces) before the token.
- A regression case: token separated by 20+ bytes of *non-whitespace* text
  still fails to match (the window must not become unbounded).
- Existing `mod tests` in `src/parser.rs` all keep passing (especially the
  hyphenation and marker-gap cases).

Follow the existing pattern: call the internal functions directly with
explicit `text_lines` (see "Testing HTML Parsing Issues" in AGENTS.md).

## Out of scope

- Migrating the styling matcher onto `SourceMap` spans (the "real fix" in
  ROADMAP.md Phase 7 item 2). Do not restructure `extract_formatting` or
  retire `match_sequence`; this task only fixes the window placement.
