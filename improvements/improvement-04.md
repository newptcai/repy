# Improvement 04 — Edit bookmark labels

Status: todo

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

## Problem

Bookmarks get an auto-generated label when created and it can never be changed.
The bookmarks window (`src/ui/windows/bookmarks.rs`, backed by the `bookmarks`
table in `src/state.rs`) supports jumping and deleting only, so long-lived
bookmarks end up with stale or meaningless names.

## Goal

Press `e` on a bookmark in the Bookmarks window to edit its label in place.

## Requirements

1. `e` in the Bookmarks window opens a text-input prompt pre-filled with the
   current label. Reuse the existing single-line text-input machinery the app
   already has (search prompt / highlight-comment input in
   `src/ui/reader/mod.rs`) rather than inventing a new widget.
2. `Enter` saves: persist the new label via a new `State` method (e.g.
   `update_bookmark_label`) and refresh the visible list immediately. `Esc`
   cancels without changes. An empty submitted label keeps the old one (or is
   rejected with a toast) — never store an empty label.
3. Only the label changes: position/anchoring data of the bookmark must be
   untouched, and the edit must survive reopening the book and the app.
4. Fuzzy filtering in the window must match against the updated label.
5. Update the Help window text and README.md (bookmarks section) in the same
   commit, per the project keybinding rule.

## Tests

- Unit test in `src/state.rs` mod tests: create a bookmark, update its label,
  read it back; empty-label update is a no-op/error.
- TUI snapshot test in `src/ui/reader/snapshot_tests.rs`: add a bookmark, open
  the window, press `e`, type a new label, Enter, snapshot the renamed list.

## Out of scope

Bookmark reordering, colors/categories, editing highlight comments (already
exists), bulk operations.
