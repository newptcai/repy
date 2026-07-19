# Improvement 05 — Export reading statistics (--export-stats)

Status: todo

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

## Problem

Phase 2 added reading statistics (`reading_sessions` table,
`State::get_reading_statistics` in `src/state.rs`, Statistics window in
`src/ui/windows/statistics.rs`), but the data is trapped in the TUI popup.
Highlights already have `--export-highlights --format md|json`
(`src/cli.rs`, `src/main.rs`); statistics deserve the same treatment.

## Goal

`repy --export-stats <path>` writes reading statistics to a file, honoring the
existing `--format` flag (`json` default, `md` for Markdown), mirroring the
`--export-highlights` UX.

## Requirements

1. Add `--export-stats <PATH>` to `src/cli.rs` next to `--export-highlights`,
   sharing the existing `--format` value enum. Handle it in `src/main.rs`
   before terminal setup, exit 0 on success, non-zero with a clear message if
   the database is missing/empty.
2. JSON output: global totals plus a per-book array (book title/author where
   available from the library/history tables, total reading time, rows and
   words read, WPM, session count, first/last read dates, current streak for
   the global section). Reuse `ReadingStatistics`/`ReadingStatsTotals` rather
   than duplicating aggregation SQL where possible; extend the query layer if
   per-book breakdown needs it.
3. Markdown output: a readable report — global summary section, then a table
   of books sorted by total reading time (columns: title, time, words, WPM,
   last read). Reuse the duration formatting logic that the Statistics window
   already has (extract it somewhere shareable instead of copy-pasting).
4. Document the flag in README.md ("Other options") with a sample invocation
   and a truncated sample of the Markdown output.

## Tests

- Unit tests for both formatters given a fabricated statistics value (no
  database needed): assert key fields/rows appear, durations formatted as in
  the Statistics window.
- `assert_cmd` test in `tests/cli.rs`: `--export-stats` against a temp
  `states.db` (create via `State` API with a couple of inserted sessions, or
  point XDG dirs at a temp home as existing tests do) produces a parseable
  JSON file; and errors cleanly when there are no sessions.

## Out of scope

CSV output, charts, per-day/heatmap breakdowns, exporting from inside the TUI.
