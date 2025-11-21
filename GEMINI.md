# Gemini Porting Plan: epy to repy

This document outlines the plan for porting the Python-based epub reader `epy` to a Rust-based equivalent, `repy`.

## Porting Steps

The porting process will be broken down into the following steps:

1.  **Project Setup:**
    *   [x] Initialize a new Rust project.
    *   [x] Add core dependencies to `Cargo.toml`: `ratatui`, `crossterm`, `eyre`.
    *   [x] Research and add dependencies for epub parsing (`epub` crate seems promising) and HTML parsing (`scraper` or `html5ever`).

2.  **Basic Structure and Error Handling:**
    *   [ ] Set up the main application entry point in `src/main.rs`.
    *   [ ] Implement a global error handling solution using `eyre`.

3.  **Data Models (`src/models.rs`):**
    *   [ ] Port all data classes from `epy/src/epy_reader/models.py` to Rust structs. This includes:
        *   `Direction` (as an enum)
        *   `InlineStyle`
        *   `Key`
        *   `LettersCount`
        *   `NoUpdate` (might be replaced by `Option` or `Result`)
        *   `ReadingState`
        *   `SearchData`
        *   `TextStructure`
        *   `TocEntry`

4.  **Configuration (`src/config.rs`):**
    *   [ ] Port the `Config` class from `epy/src/epy_reader/config.py`.
    *   [ ] Port the settings from `epy/src/epy_reader/settings.py`.
    *   [ ] Implement loading/saving of configuration from/to a file (e.g., TOML or JSON).

5.  **Application State (`src/state.rs`):**
    *   [ ] Port the `State` class from `epy/src/epy_reader/state.py`.
    *   [ ] Implement a simple database using `rusqlite` to store bookmarks and reading history.

6.  **Ebook Parsing (`src/ebook.rs`, `src/parser.rs`):**
    *   [ ] Create an `Ebook` trait to handle different ebook formats.
    *   [ ] Implement an `Epub` struct that implements the `Ebook` trait, using the `epub` crate.
    *   [ ] Port the `parse_html` function from `epy/src/epy_reader/parser.py`. This will involve using an HTML parsing crate to extract text and structure from the epub's HTML content.

7.  **Terminal UI (`src/ui/`):**
    *   [ ] Create a `ui` module to hold all TUI-related code.
    *   [ ] **Main Reader (`src/ui/reader.rs`):**
        *   [ ] Create a `Reader` struct to manage the application's main state and logic.
        *   [ ] Implement the main event loop, handling user input from `crossterm`.
    *   [ ] **Content View (`src/ui/board.rs`):**
        *   [ ] Implement a `Board` widget (or similar) that is responsible for rendering the book's text content using `ratatui`.
    *   [ ] **Dialogs/Windows (`src/ui/windows/`):**
        *   [ ] Create separate modules for each dialog/window:
        *   [ ] Table of Contents
        *   [ ] Metadata display
        *   [ ] Help window
        *   [ ] Bookmarks management
        *   [ ] Library view
        *   [ ] Search input and results

8.  **Command-Line Interface (`src/cli.rs`):**
    *   [ ] Port the argument parsing logic from `epy/src/epy_reader/cli.py` using the `clap` crate.
    *   [ ] Handle starting the TUI or dumping book content based on arguments.

9.  **Utilities (`src/utils.rs`):**
    *   [ ] Port the helper functions from `epy/src/epy_reader/utils.py` and `epy/src/epy_reader/lib.py` to a `utils` module.

10. **Integration (`src/main.rs`):**
    *   [ ] Tie all the modules together in the `main` function.
    *   [ ] Initialize the configuration and state.
    *   [ ] Parse command-line arguments.
    *   [ ] Set up the terminal for `ratatui`.
    *   [ ] Create and run the main `Reader` application.
    *   [ ] Ensure graceful shutdown and terminal restoration.
