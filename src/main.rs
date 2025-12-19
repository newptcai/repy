use repy::{
    cli::Cli,
    config::Config,
    logging::{self, LogLevel},
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
        println!("ebook: {:?}", cli.ebook);
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
    if cli.dump && cli.ebook.is_empty() {
        // Dump mode without an ebook path.
        println!("Content dumping not yet implemented");
        return Ok(());
    }

    if !cli.ebook.is_empty() {
        let filepath = &cli.ebook[0]; // Take the first ebook path
        if cli.dump {
            // Dump content mode
            dump_content(filepath)?;
        } else {
            // TUI mode with a file
            if !std::path::Path::new(filepath).exists() {
                eprintln!("Warning: Ebook path does not exist: {}", filepath);
                return Ok(());
            }
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

fn run_tui_with_file(filepath: &str, config: Config) -> Result<()> {
    let mut reader = Reader::new(config)?;
    if let Err(e) = reader.load_ebook(filepath) {
        eprintln!("Warning: Could not load ebook: {}", e);
    }
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
