# Improvement 01 — Graceful fallback when configuration.json is broken

Status: done

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

## Problem

When `configuration.json` exists but fails to parse, `src/main.rs` falls into
`run_tui_with_default_config()` (around `src/main.rs:261`), which is a stub: it
prints "TUI with default configuration not yet implemented" and exits. A user
with one bad character in their config is locked out of the reader entirely.

## Goal

A broken config must never prevent reading. On config load failure, start the
TUI with default settings and tell the user what is wrong.

## Requirements

1. Replace the stub: build a pure-default `Config` (do NOT re-read the broken
   file) and run the normal TUI path with it, honoring the CLI arguments the
   user passed (file/history/resume behavior should work the same as with a
   valid config).
2. Capture the underlying parse/load error (serde gives line/column info) and
   surface it inside the TUI as a sticky warning toast on startup, e.g.:
   `Config invalid, using defaults: <path>: <error>`. Also log it via the
   existing logging module.
3. The session must never overwrite the user's broken config file. If any code
   path would persist settings back to the config file (e.g. the Settings
   window), suppress the write in fallback mode and show a toast explaining
   that the config was not saved because the file on disk is invalid.
4. Per-book state (SQLite) keeps working normally in fallback mode.

## Tests

- Unit test(s) in `src/config.rs`: loading a malformed JSON file returns an
  error whose message contains the file path and the serde detail.
- A test that the fallback path constructs a default `Config` without touching
  the broken file (assert file mtime/content unchanged is fine).
- If practical, a TUI snapshot test (see `src/ui/reader/snapshot_tests.rs`)
  showing the startup warning toast; skip if wiring a broken-config fixture
  into the snapshot harness is disproportionate.

## Out of scope

Config schema migration, partial/lenient parsing of half-valid configs,
interactive config repair.
