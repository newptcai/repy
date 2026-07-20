# Improvement 07 — Clean selection text via SourceMap (yank + dictionary)

Status: todo

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

Context: ROADMAP.md Phase 7 item 3, first slice. Improvements 08 and 09 will
reuse the mapping helper introduced here — get its shape right.

## Problem

Yanked text and dictionary queries are read straight off the rendered grid:
`yank_selection` (`src/ui/reader/mod.rs:8800`) and `dictionary_lookup`
(`src/ui/reader/mod.rs:8925`) both call
`Board::get_selected_text_range(anchor, cursor)` (`src/ui/board.rs:753`),
which concatenates character slices of the wrapped `text_lines`. So the
output contains layout artifacts:

- justification padding (multiple spaces between words),
- two-column first-line indents (`indented` paragraph style),
- trailing wrap hyphens from the hyphenation pass (`fix-` / `ture` yanks as
  `fix-\nture`),
- hard line breaks inside a paragraph.

Since Phase 7 item 0, every chapter's `TextStructure` carries a
`SourceMap` (`src/models.rs:431`): `row_spans` gives each chapter-local
wrapped row a `[start, end)` char span into `source_text` (the normalized
chapter source), with `offset_for_row` / `row_for_offset` projections. The
reader holds per-chapter structures in `chapter_text_structures`
(`src/ui/reader/mod.rs:894`) and already maps global rows to
`(content_index, source_offset)` via `source_position_for_row`
(`src/ui/reader/mod.rs:7158`).

What is missing is *column* resolution: selections have `(row, col)`
endpoints (char-index columns, inclusive end — see the current slicing in
`get_selected_text_range`), and rendered columns ≠ source offsets because of
the artifacts above.

## Goal

Yank and dictionary lookup return text read from `source_map.source_text`:
single spaces between words, no indent padding, no wrap hyphens, real
characters otherwise identical to the source. Multi-row selections within a
paragraph join into flowing text (newlines only where the source itself has
them). Selection *behavior* (what rows/cols the user picks) is unchanged.

## Requirements

1. **Mapping helper (the reusable core).** Add a helper that converts a
   rendered position within one row to an offset in that row's source span —
   suggested home: `impl SourceMap` in `src/models.rs`, e.g.
   `offset_at(row: usize, col: usize, bias: Bias) -> usize` where `Bias`
   distinguishes a start endpoint (round forward) from an inclusive end
   endpoint (round back, then the caller adds 1 to make it exclusive).
   Implement it as a lockstep walk over the rendered row's chars and the
   row's span chars in `source_text`:
   - rendered char == source char → advance both;
   - both are whitespace → consume the whole rendered whitespace run and a
     single source whitespace char (normalization collapses runs);
   - rendered char is whitespace or `-` but the source char is not →
     advance rendered only (this absorbs indent columns, justification
     padding, and the synthetic trailing wrap hyphen);
   - anything else mismatching → stop and clamp (defensive; add a debug
     log via `src/logging.rs` so drift is visible during development).
   The walk is O(row length); no caching needed.
2. **Board extraction.** Add `Board::get_selected_source_text(start, end,
   &[TextStructure]-like access)` or, if cleaner, put the row iteration in
   the reader where `chapter_text_structures` already lives, and keep the
   Board out of it. Rules:
   - Resolve each endpoint through requirement 1's helper (global row →
     chapter + local row via the existing `content_index_for_row` /
     `source_position_for_row` machinery).
   - A selection entirely inside one chapter yields
     `source_text[start_offset..end_offset]` — one slice, done. This is the
     common case and must not re-join per-row fragments.
   - A selection spanning a chapter boundary joins the per-chapter slices
     with a single `\n`.
   - Skip synthetic rows exactly as today (`typography_spacing_rows`,
     chapter-break padding rows have empty spans, so the offset math already
     handles them — verify, don't special-case unless a test proves the
     need).
3. **Call sites.** `yank_selection` and `dictionary_lookup` switch to the new
   extraction. `get_selected_text_range` itself stays (other consumers and
   the highlight-creation path may rely on rendered text); do not change its
   behavior.
4. Empty results keep the current UX (yank silently returns, dictionary
   reopens the reader window).

## Tests

Codex writes the tests. Cover at least, using a fixture chapter parsed at a
narrow width with justification on and `indented` paragraph style:

- Yanking a phrase that spans a justified gap yields single-spaced text.
- Yanking across a wrap boundary with a hyphenated word yields the unbroken
  word (no `-`, no newline).
- Yanking across the first-line indent excludes the indent padding.
- Single-word selection maps exact endpoints (no neighbor characters) —
  test both bias directions at word boundaries.
- A selection across a chapter boundary joins with exactly one `\n`.
- `offset_at` unit tests directly on a hand-built `SourceMap` (row text vs
  source text with known artifacts), including the defensive-clamp path.
- Snapshot tests: none needed unless rendering changes (it must not).

## Out of scope

- TTS text (improvement-08) and search (improvements 09/10).
- Changing how selections are *made* or how highlights anchor
  (`src/annotations.rs` already normalizes independently).
- Removing `get_selected_text_range`.
