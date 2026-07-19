# Improvement 02 — Shell completion generation (--completions)

Status: done

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
5. The generated script must be a plain, self-contained static script written
   to stdout with no side effects and nothing else on stdout (no banners or
   log lines), so that redirecting it to a file and sourcing that file from a
   bash startup script works as-is. In the README bash example, show the
   save-to-file-and-source pattern (not only eval/source-on-the-fly), e.g.:
   `repy --completions bash > ~/.local/share/bash-completion/completions/repy`.

## Tests

- `assert_cmd` tests in `tests/cli.rs` (follow the existing pattern there):
  - `repy --completions bash` exits 0 and stdout contains `repy`;
  - same for `zsh` and `fish`;
  - an invalid shell name exits non-zero with a clap error.

## Out of scope

Man page generation, packaging/distribution changes, dynamic completion of
book titles from the history database.

## Follow-up (NOT part of this task — do not do this in the repy repo)

The maintainer keeps personal completions as static per-tool files in a
separate dotfiles repo (`$SRC_DIR/bash/bash_completion_<tool>`), sourced behind
file-exists guards from a `bash_completion` dispatcher. After this task lands,
installation there is done manually outside this repo:
`repy --completions bash > $SRC_DIR/bash/bash_completion_repy` plus a guard
stanza in the dispatcher. This is only context explaining requirement 5;
nothing in the dotfiles repo should be touched here.
