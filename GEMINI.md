# Gemini Porting Plan: epy to repy

This document outlines the plan for porting the Python-based epub reader `epy` to a Rust-based equivalent, `repy`.

## Key Porting Considerations & Challenges

Porting from a dynamically-typed language like Python to a statically-typed, compiled language like Rust involves more than a direct translation of code. The following points highlight key architectural and idiomatic shifts to consider:

### 1. From Dynamic to Static Typing
*   **Data Structures:** Python's flexible `dict`s and dynamic objects must be mapped to explicit Rust `struct`s and `enum`s. This requires analyzing the data flow in `epy` to define clear, typed contracts.
*   **Signaling:** Python patterns like the `NoUpdate` class are used for signaling. In Rust, these should be replaced with idiomatic types like `Option<T>` (to represent presence or absence) and `Result<T, E>` (for operations that can succeed or fail), which provide compile-time safety.

### 2. Error Handling Strategy
*   The port will move from Python's exception-based error handling (`try...except`) to Rust's `Result`-based model.
*   Using the `eyre` crate, as planned, is a good choice. It will allow for creating a centralized, context-rich error reporting system that is more ergonomic than manually propagating `Result` types everywhere.

### 3. Object-Oriented to Trait-Based Design
*   `epy` uses class inheritance for polymorphism (e.g., for different ebook formats or text-to-speech speakers).
*   The idiomatic Rust approach is to use `trait`s. Defining an `Ebook` trait, for instance, will allow different parsers (for Epub, Mobi, etc.) to be used interchangeably by the application logic.

### 4. State Management and Ownership
*   This is a primary challenge in Rust TUI applications. The central application `State` needs to be safely accessed and modified by various components (UI, event handler, etc.).
*   Patterns involving `Rc<RefCell<T>>` will likely be necessary to allow shared, mutable access to state in a single-threaded context, satisfying the borrow checker. An alternative could be a more centralized message-passing architecture where components send events to update a single state owner.

### 5. Dependency Ecosystem Mapping
*   Each Python dependency from `pyproject.toml` must be mapped to a suitable Rust crate.
*   **Key Mappings:**
    *   TUI: `tui-rs` + `textwrap` -> `ratatui`
    *   Terminal Backend: `pyte` -> `crossterm`
    *   HTML Parsing: `beautifulsoup4` -> `scraper`
    *   CLI Parsing: `argparse` -> `clap`
*   The HTML parsing logic in `epy/src/epy_reader/parser.py` is particularly complex and will require careful implementation using the chosen Rust HTML parsing crate.

### 6. UI and Event Loop
*   The `ratatui` framework works by re-rendering the entire UI on each "tick" or event.
*   The core of the application will be a main loop that:
    1.  Waits for an input event from `crossterm`.
    2.  Updates the application state based on the event.
    3.  Draws the entire UI based on the new state.
*   This is a different model from many other UI paradigms and will be a foundational piece of the architecture.

## Porting Steps

The porting process will be broken down into the following steps:

1.  **Project Setup:**
    *   [x] Initialize a new Rust project.
    *   [x] Add core dependencies to `Cargo.toml`: `ratatui`, `crossterm`, `eyre`.
    *   [x] Research and add dependencies for epub parsing (`epub` crate seems promising) and HTML parsing (`scraper` or `html5ever`).

2.  **Basic Structure and Error Handling:**
    *   [x] Set up the main application entry point in `src/main.rs`.
    *   [x] Implement a global error handling solution using `eyre`.

3.  **Data Models (`src/models.rs`):**
    *   [x] Port all data classes from `epy/src/epy_reader/models.py` to Rust structs. This includes:
        *   [x] `Direction` (as an enum)
        *   [x] `InlineStyle`
        *   [x] `Key` (functionality to be handled by `crossterm`'s native key handling)
        *   [x] `LettersCount`
        *   [x] `NoUpdate` (might be replaced by `Option` or `Result`)
        *   [x] `ReadingState`
        *   [x] `SearchData`
        *   [x] `TextStructure`
        *   [x] `TocEntry`

4.  **Configuration (`src/config.rs`):**
    *   [x] Port the `Config` class from `epy/src/epy_reader/config.py`.
    *   [x] Port the settings from `epy/src/epy_reader/settings.py`.
    *   [x] Implement loading/saving of configuration from/to a file (e.g., TOML or JSON).

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
