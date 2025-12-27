# `repy`

Rust port of the awesome CLI ebook reader [`epy`](https://github.com/wustho/epy).

The goal is to keep the reading experience and keybindings familiar while improving
performance, robustness, and portability by using Rust and a fully self-contained
SQLite implementation.

## Status

This is work-in-progress. Core TUI reading, configuration loading, and basic
library state are implemented; some `epy` features (TTS, external tools, etc.)
are not yet available.

## Installation

`repy` is a normal Rust binary:

```sh
cargo install --path .
```

The bundled `rusqlite` feature is enabled, so no system-wide `libsqlite3`
installation is required; SQLite is compiled and linked as part of the build.

## Usage

Basic usage mirrors `epy`:

- `repy /path/to/book.epub` — open a book in the TUI.
- `repy` — start the TUI without an explicit file:
  - If there is a reading history, `repy` reopens the last-read book at the
    last saved position.
  - Otherwise, it starts in the reader UI without a book loaded.

Keyboard controls are intentionally close to `epy` and are documented in the
help window inside the TUI. A few important ones:

- `j` / `k` — scroll down / up.
- `h` / `l` — page up / down.
- `Ctrl+o` / `Ctrl+i` — jump back / forward in position history.
- `t` — open the table of contents.
- `m` — open the bookmarks window (`a` to add, `d` to delete, `Enter` to jump).
- `r` — open the Library/history window:
  - `j` / `k` to select an entry.
  - `Enter` to open the selected book.
  - `d` to delete the selected history entry (and its saved reading state).

## Configuration

Configuration is stored as JSON, in the same locations `epy` uses but under
the `repy` directory:

- Linux / macOS:
  - `~/.config/repy/configuration.json` if `XDG_CONFIG_HOME` is set, or if the
    `~/.config/repy` directory already exists.
  - Otherwise: `~/.repy/configuration.json`.
- Windows:
  - `%USERPROFILE%\.repy\configuration.json`.

The configuration file controls settings such as colors, width, mouse support,
and keybindings. If the file does not exist, `repy` creates it with sensible
defaults on first run.

## Database and Reading State

`repy` stores reading history, last positions, and bookmarks in a SQLite
database managed entirely from Rust (no external `sqlite3` binary or library
is required).

The database file is located next to the configuration file:

- Linux / macOS:
  - `~/.config/repy/states.db` or `~/.repy/states.db`.
- Windows:
  - `%USERPROFILE%\.repy\states.db`.

Internally, the database contains three tables:

- `reading_states` — last known position per book:
  - `filepath`, `content_index`, `textwidth`, `row`, `rel_pctg`.
- `library` — reading history and metadata:
  - `filepath`, `last_read`, `title`, `author`, `reading_progress`.
- `bookmarks` — named bookmarks per book:
  - `id`, `filepath`, `name`, plus a copy of the reading position fields.

On quit (`q` from the reader window), `repy`:

- Saves the current `ReadingState` for the open book into `reading_states`.
- Updates the corresponding `library` entry (including `last_read` and
  `reading_progress`).

When you open a book (either by passing a path, or by selecting from the
library, or via the “last book” behavior when starting with no arguments),
`repy`:

- Looks up any existing `reading_states` entry for that `filepath`.
- Restores your last position and relative progress where possible.
- Reloads any stored bookmarks for that book.

## Contributing

This port is still evolving. Bug reports, small focused patches, and feedback on
feature parity with `epy` are very welcome.
