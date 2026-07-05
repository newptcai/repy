use repy::{
    annotations,
    cli::{Cli, ExportFormat},
    config::Config,
    ebook::{Ebook, Epub},
    logging::{self, LogLevel},
    state::State,
    ui::reader::Reader,
};

use clap::Parser;
use eyre::Result;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let log_level = if cli.debug || cli.verbose > 1 {
        LogLevel::Debug
    } else if cli.verbose > 0 {
        LogLevel::Info
    } else {
        LogLevel::Warn
    };
    logging::init(log_level);

    if cli.debug {
        logging::debug(format!("CLI options: {:?}", cli));
    }

    if std::env::var_os("REPY_CLI_ECHO").is_some() {
        println!("history: {}", cli.history);
        println!("dump: {}", cli.dump);
        println!("export_highlights: {:?}", cli.export_highlights);
        println!("ebook: {:?}", cli.ebook);
        return Ok(());
    }

    if let Some(book) = cli.export_highlights.as_ref() {
        export_highlights(book, cli.format)?;
        return Ok(());
    }

    if cli.history {
        return print_history();
    }

    // Load configuration
    let config = match cli.config.as_ref() {
        Some(filepath) => {
            logging::info(format!("Using config file: {}", filepath.display()));
            Config::load_from(filepath.to_path_buf())
        }
        None => Config::new(),
    };
    let config = match config {
        Ok(config) => config,
        Err(err) => {
            logging::warn(format!("Could not load configuration: {}", err));
            eprintln!("Starting with default settings");
            // We can't create a Config manually due to private fields,
            // so we'll use a placeholder approach for now
            return run_tui_with_default_config();
        }
    };

    // Handle different CLI modes
    if cli.dump {
        let Some(arg) = cli.ebook.first() else {
            eprintln!("Error: provide an ebook path, history number, or pattern to dump");
            std::process::exit(1);
        };
        return dump_content(&resolve_ebook_arg(arg)?);
    }

    if let Some(arg) = cli.ebook.first() {
        match resolve_ebook_arg(arg) {
            Ok(filepath) => run_tui_with_file(&filepath, config)?,
            Err(err) => {
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        }
    } else {
        // TUI mode without a file (reopen last-read book)
        run_tui(config)?;
    }

    Ok(())
}

/// Resolve the EBOOK argument as an existing path, a 1-based reading-history
/// number, or a case-insensitive pattern matched against history entries
/// (most recently read match wins).
fn resolve_ebook_arg(arg: &str) -> Result<String> {
    if std::path::Path::new(arg).exists() {
        return Ok(arg.to_string());
    }

    let items = State::new()?.get_from_history()?;
    if let Ok(number) = arg.parse::<usize>() {
        if (1..=items.len()).contains(&number) {
            return Ok(items[number - 1].filepath.clone());
        }
        eyre::bail!(
            "history number {} is out of range (history has {} entries)",
            number,
            items.len()
        );
    }

    let needle = arg.to_lowercase();
    let matched = items.iter().find(|item| {
        item.filepath.to_lowercase().contains(&needle)
            || item
                .title
                .as_deref()
                .is_some_and(|t| t.to_lowercase().contains(&needle))
            || item
                .author
                .as_deref()
                .is_some_and(|a| a.to_lowercase().contains(&needle))
    });
    match matched {
        Some(item) => Ok(item.filepath.clone()),
        None => eyre::bail!("'{}' is not a file and no history entry matches it", arg),
    }
}

fn print_history() -> Result<()> {
    let items = State::new()?.get_from_history()?;
    if items.is_empty() {
        println!("Reading history is empty.");
        return Ok(());
    }
    for (index, item) in items.iter().enumerate() {
        let progress = item
            .reading_progress
            .map(|p| format!("{:>3.0}%", p * 100.0))
            .unwrap_or_else(|| "  --".to_string());
        let title = item.title.as_deref().filter(|t| !t.is_empty()).map_or_else(
            || {
                std::path::Path::new(&item.filepath)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| item.filepath.clone())
            },
            ToString::to_string,
        );
        let author = item
            .author
            .as_deref()
            .filter(|a| !a.is_empty())
            .map(|a| format!(" - {}", a))
            .unwrap_or_default();
        println!("{:>3}  {}  {}{}", index + 1, progress, title, author);
        println!("     {}", item.filepath);
    }
    Ok(())
}

fn run_tui(config: Config) -> Result<()> {
    let mut reader = Reader::new(config)?;
    // When started without an explicit file, mimic `epy` by
    // reopening the last-read book at its saved position if available.
    reader.load_last_ebook_if_any()?;
    reader.run()
}

fn run_tui_with_file(filepath: &str, config: Config) -> Result<()> {
    let mut reader = Reader::new(config)?;
    if let Err(e) = reader.load_ebook(filepath) {
        eprintln!("Warning: Could not load ebook: {}", e);
    }
    reader.run()
}

fn dump_content(filepath: &str) -> Result<()> {
    use std::io::Write;

    let mut epub = Epub::new(filepath);
    epub.initialize()?;
    let structures = epub.get_all_parsed_content(80, None)?;

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for (index, structure) in structures.iter().enumerate() {
        if index > 0 && writeln!(out).is_err() {
            return Ok(()); // Stop quietly on a closed pipe (e.g. piped to head)
        }
        for line in &structure.text_lines {
            if writeln!(out, "{}", line).is_err() {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn export_highlights(filepath: &std::path::Path, format: ExportFormat) -> Result<()> {
    let path = filepath.to_string_lossy();
    let mut epub = Epub::new(&path);
    epub.initialize()?;
    let identity = annotations::derive_book_identity(&mut epub)?;
    let db = State::new()?;
    let mut highlights = db.list_highlights(&identity.book_id)?;

    match format {
        ExportFormat::Json => {
            let payload = serde_json::json!({
                "book": identity,
                "highlights": highlights,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        ExportFormat::Md => {
            highlights.sort_by_key(|h| (h.content_index, h.approx_offset));
            print!("{}", highlights_to_markdown(&epub, &highlights));
        }
    }
    Ok(())
}

/// Render highlights as Markdown grouped by chapter, in reading order.
fn highlights_to_markdown(epub: &Epub, highlights: &[repy::models::Highlight]) -> String {
    use std::fmt::Write;

    let meta = epub.get_meta();
    let mut out = String::new();
    let title = meta.title.as_deref().unwrap_or("Untitled");
    writeln!(out, "# Highlights: {}", title).unwrap();
    if let Some(author) = meta.creator.as_deref().filter(|a| !a.is_empty()) {
        writeln!(out, "\nAuthor: {}", author).unwrap();
    }

    let mut current_chapter: Option<usize> = None;
    for highlight in highlights {
        if current_chapter != Some(highlight.content_index) {
            current_chapter = Some(highlight.content_index);
            let label = epub
                .toc_entries()
                .iter()
                .find(|entry| entry.content_index == highlight.content_index)
                .map(|entry| entry.label.clone())
                .unwrap_or_else(|| format!("Chapter {}", highlight.content_index + 1));
            writeln!(out, "\n## {}", label).unwrap();
        }
        writeln!(out).unwrap();
        for line in highlight.exact.lines() {
            writeln!(out, "> {}", line).unwrap();
        }
        if let Some(comment) = highlight
            .comment
            .as_deref()
            .filter(|c| !c.trim().is_empty())
        {
            writeln!(out, "\nNote: {}", comment.trim()).unwrap();
        }
        writeln!(
            out,
            "\n*{}*",
            highlight.created_at.format("%Y-%m-%d %H:%M")
        )
        .unwrap();
    }
    out
}

fn run_tui_with_default_config() -> Result<()> {
    // TODO: Implement a fallback TUI with default config
    println!("TUI with default configuration not yet implemented");
    Ok(())
}
