# Improvement 10 — Multi-row search hits as first-class ranges

Status: done

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

Context: ROADMAP.md Phase 7 item 1, second slice. **Depends on
improvement-09** (source-text matching with hits carrying
`(content_index, m_start, m_end)`). Do not start before 09 is merged.

## Problem

After improvement-09, a hit spanning several rendered rows highlights on all
of them but is still *modeled* as "a result at its first row":

- the distinct current-hit style (added in Phase 1 search upgrades) is
  applied per-row, so only the anchor row of the current hit is
  distinguished from other matches;
- `search_next`/`search_previous` position on the anchor row, which is fine,
  but the "match N/M" counter and current-hit bookkeeping can desynchronize
  when several hits share an anchor row (two hits starting on one row) —
  verify and fix whatever the row-keyed bookkeeping gets wrong;
- the results-window entries show first-row-only ranges.

## Goal

A search hit is a range, everywhere: the whole hit (all its rows/columns)
renders with the current-hit style when selected, navigation counts hits
(not rows), and the results window shows one entry per hit with a clean
source-text preview. Behavior for single-row hits is pixel-identical to
improvement-09.

## Requirements

1. **Model.** Promote the hit to the primary structure, e.g.
   `SearchHit { content_index, start_offset, end_offset, per_row: Vec<(global_row, col_start, col_end)> }`
   (or keep `SearchResult` and grow it — pick whichever leaves fewer
   parallel collections; `ui_state.search_matches` may remain as a derived
   render-side index but must be *derived from* the hit list in one place,
   not maintained separately).
2. **Current-hit rendering.** `src/ui/board.rs` receives which
   `(row, range)` pairs belong to the currently selected hit (today it
   derives "current" from `selected_search_result` + row, around
   `src/ui/board.rs:190`). All rows of the current hit get the current-hit
   style; other hits keep the normal match style. Rows visible while other
   rows of the same hit are scrolled off-screen must still style correctly.
3. **Navigation.** `n`/`p`/`N` iterate the hit list. Position lands on the
   hit's first row (unchanged). The `match N/M` counter counts hits.
   Determining "which hit am I on" after a manual scroll uses the hit whose
   row range contains (or is nearest below) the current row — same rule the
   row-based code used, now on ranges.
4. **Results window.** One row per hit; the preview is the improvement-09
   source-text excerpt (verify it is used and wrap-boundary hits show an
   unbroken preview). Jumping from the window selects that hit (current-hit
   styling included) — this should already fall out of req. 1–3; test it.
5. **Jump history** records entering a hit exactly as before (one entry per
   navigation, no per-row duplicates).
6. Visual-mode `/`-search stays untouched; its ranges merge into rendering
   exactly as in improvement-09.

## Tests

Codex writes the tests. Cover at least:

- A 3-row hit: selecting it styles all three rows as current; `n` moves to
  the next *hit*, not the next row of the same hit.
- Two hits starting on the same row: counter shows 2 distinct positions;
  `n` cycles both.
- Wrap-boundary hit preview in the results window shows the unbroken phrase.
- Counter/current-hit stay consistent after scrolling away and pressing `n`.
- Snapshot tests: current-hit styling structure on the 80×24 harness for a
  multi-row hit (structure only — `TestBackend` renders no colors, so assert
  on counter text and layout; the per-row style split itself needs a unit
  test at the span-building level in `src/ui/board.rs`).

## Out of scope

- Any further search UX (fuzzy, history changes, cross-book search).
- Visual-mode `/`-search internals.
- Performance work beyond what improvement-09 established.
