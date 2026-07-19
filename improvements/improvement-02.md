# Improvement 02 — Shell completion generation (--completions)

Status: todo

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

## Problem

repy has a reasonably rich CLI (`src/cli.rs`: `-r`, `-d`, `--export-highlights`,
`--format`, history-number/pattern launch) but ships no shell completions.

## Goal

`repy --completions <shell>` prints a completion script to stdout for bash,
zsh, fish, and powershell, so users can install it with e.g.
`repy --completions fish > ~/.config/fish/completions/repy.fish`.

## Requirements

1. Add the `clap_complete` crate (match the existing clap 4.x version) and a
   `--completions <SHELL>` value-enum option in `src/cli.rs`.
2. When the flag is given, `src/main.rs` generates the script to stdout and
   exits 0 before any config/terminal/database work happens.
3. The flag must not conflict with the existing positional/pattern launch
   logic; `repy --completions bash` alone must not be treated as opening a
   book.
4. Document it in README.md under "Other options", including one-line install
   examples for bash, zsh, and fish.

## Tests

- `assert_cmd` tests in `tests/cli.rs` (follow the existing pattern there):
  - `repy --completions bash` exits 0 and stdout contains `repy`;
  - same for `zsh` and `fish`;
  - an invalid shell name exits non-zero with a clap error.

## Out of scope

Man page generation, packaging/distribution changes, dynamic completion of
book titles from the history database.
