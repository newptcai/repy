# Improvement 09 — Search matches on source text (engine swap)

Status: done

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

Context: ROADMAP.md Phase 7 item 1, first slice. **Depends on
improvement-07** (needs the offset→column projection; if 07 shipped only
`offset_at`, add the inverse `col_at` as specified in improvement-08 req. 3 —
whichever of 08/09 lands first introduces it). Improvement-10 builds on this
task; keep the seams it needs (see Out of scope).

## Problem

`scan_search_matches` (`src/ui/reader/mod.rs:7434`) runs the user's regex
over each *rendered* line independently. Two failure classes:

- justification: `foo bar` never matches the stretched `foo  bar`
  (and users cannot reasonably write `foo\s+bar` for every space);
- wrap boundaries: a phrase split across two rows (or hyphenated,
  `fix-`/`ture`) never matches at all.

Since Phase 7 item 0, each chapter has `source_map.source_text` — the
normalized source the phrase actually lives in — plus `row_for_offset`.

## Goal

The regex runs once per chapter over `source_map.source_text`. Hits are
projected back to rendered rows/columns for highlighting and navigation.
The externally visible data model stays row-based in this task: a hit that
spans multiple rows highlights correctly on every affected row, but
navigation and the results window still treat it as one result anchored at
its first row. (Range-aware navigation/rendering is improvement-10.)

## Requirements

1. **Rewrite `scan_search_matches`.** For each chapter in
   `chapter_text_structures` (with its global `content_start_rows` offset):
   run `regex.find_iter(&source_map.source_text)`; for each match
   `[m_start, m_end)`:
   - rows touched: `row_for_offset(m_start)` through
     `row_for_offset(m_end - 1)` (chapter-local; add the chapter start for
     global rows);
   - for each touched row, project the overlap of `[m_start, m_end)` with
     the row's span into rendered columns via the `col_at` /
     `offset_at`-inverse helper, producing one `(start, end)` range per row.
   Guard against pathological matches (e.g. `.*` matching a whole chapter):
   cap per-row work by intersecting with row spans — the algorithm above is
   already linear; just make sure an empty-width match advances (skip
   zero-length matches exactly the way the old per-line code implicitly
   did — verify what `find_iter` does with them and keep behavior sane).
2. **Units decision (do this first).** Today `search_matches` stores *byte*
   ranges from the rendered-line regex, while the visual-mode `/`-search
   ranges are *char* based, and both feed `combined_search_ranges` →
   `build_line_spans` in `src/ui/board.rs` (~line 259). Determine what
   `build_line_spans` actually indexes with (read it, don't guess). Pick
   char indices as the canonical unit for the new projection (that is what
   `col_at` produces), and convert at the boundary if `build_line_spans`
   wants bytes. If the existing merge is silently mixing units on non-ASCII
   lines, note it in the commit message — fixing the visual-mode side is
   out of scope unless it is a one-line conversion.
3. **Keep the row-based model.** `ui_state.search_matches` stays
   `HashMap<global_row, Vec<(start, end)>>`; a multi-row hit inserts a range
   into each touched row. `SearchResult { line, ranges, preview }`
   (`src/ui/reader/mod.rs:659`): one entry per *hit* (not per row), `line` =
   first touched global row, `ranges` = that first row's ranges, `preview` =
   a source-text excerpt of the match with a few words of context (this
   improves the results window for free — previews no longer end mid-word
   at a wrap boundary). Internally, also store each hit's
   `(content_index, m_start, m_end)` alongside — improvement-10 needs it;
   a parallel `Vec` or an extra field on `SearchResult` is fine.
4. **All three entry points swap engines**: `execute_search`
   (`src/ui/reader/mod.rs:7377`), the repeat-search path around
   `src/ui/reader/mod.rs:7483`, and `update_incremental_search`
   (`src/ui/reader/mod.rs:7462`). Incremental search must stay responsive:
   per keystroke it is one regex pass per chapter over source text —
   comparable to today's per-line pass; do not add caching unless a
   measurement in the PR shows it is needed.
5. **Navigation semantics unchanged**: `search_next`/`search_previous`
   (`src/ui/reader/mod.rs:7531`/`7552`) keep operating on the
   `SearchResult` list by index/`line`. `n` from inside a multi-row hit's
   later rows may behave as if positioned at the hit's first row — acceptable
   for this slice.
6. **Search history, smartcase/escape helpers, and the search window UI keep
   working unchanged** (`build_visual_search_regex` is visual-mode only —
   leave visual-mode `/`-search alone entirely).

## Tests

Codex writes the tests. Cover at least:

- `foo bar` matches across a justified gap (fixture parsed with
  justification on); highlight ranges land on the rendered `foo` and `bar`.
- A phrase split across a wrap boundary matches; both rows get ranges;
  one `SearchResult` anchored at the first row.
- A hyphen-wrapped word (`fix-`/`ture`) matches the pattern `fixture`.
- Case-insensitive and regex-metachar patterns behave as before on
  single-row hits (regression).
- Zero-length-match pattern (e.g. `x*`) terminates and produces sane output.
- Non-ASCII line: highlight columns are correct (units decision, req. 2).
- Snapshot test: search highlight + match counter on the 80×24 harness
  (extend `src/ui/reader/snapshot_tests.rs`; structure only, per the
  determinism rules in AGENTS.md).

## Out of scope

- Range-aware `n`/`p`, current-hit styling spanning rows, and results-window
  multi-row previews — improvement-10.
- Visual-mode `/`-search internals.
- Search history/fuzzy/incremental UX changes.
