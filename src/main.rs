use repy::{
    cli::Cli,
    config::Config,
    ui::reader::Reader,
};

use clap::Parser;
use eyre::Result;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load configuration
    let config = match Config::new() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Warning: Could not load configuration: {}", err);
            eprintln!("Starting with default settings");
            // We can't create a Config manually due to private fields,
            // so we'll use a placeholder approach for now
            return run_tui_with_default_config();
        }
    };

    // Handle different CLI modes
    if !cli.ebook.is_empty() {
        let filepath = &cli.ebook[0]; // Take the first ebook path
        if cli.dump {
            // Dump content mode
            dump_content(filepath)?;
        } else {
            // TUI mode with a file
            run_tui_with_file(filepath, config)?;
        }
    } else if cli.history {
        // Show library/history mode
        println!("Library/history view not yet implemented");
    } else {
        // TUI mode without a file (show library)
        run_tui(config)?;
    }

    Ok(())
}

fn run_tui(config: Config) -> Result<()> {
    let mut reader = Reader::new(config)?;
    reader.run()
}

fn run_tui_with_file(_filepath: &str, config: Config) -> Result<()> {
    // TODO: Load ebook file and pass to reader
    // For now, just start the TUI
    let mut reader = Reader::new(config)?;
    reader.run()
}

fn dump_content(_filepath: &str) -> Result<()> {
    // TODO: Implement content dumping
    println!("Content dumping not yet implemented");
    Ok(())
}

fn run_tui_with_default_config() -> Result<()> {
    // TODO: Implement a fallback TUI with default config
    println!("TUI with default configuration not yet implemented");
    Ok(())
}
