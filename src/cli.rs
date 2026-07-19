use clap::{ArgAction, Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ExportFormat {
    Json,
    Md,
}

#[derive(Parser, Debug)]
#[clap(
    name = "repy",
    version,
    about = "A Rust port of the awesome CLI Ebook Reader `epy` by `wustho`.",
    long_about = None
)]
pub struct Cli {
    /// Print reading history and exit
    #[clap(short = 'r', long)]
    pub history: bool,

    /// Dump the parsed text content of the ebook to stdout
    #[clap(short, long)]
    pub dump: bool,

    /// Export persisted highlights for an ebook
    #[clap(long, value_name = "BOOK")]
    pub export_highlights: Option<PathBuf>,

    /// Output format for --export-highlights
    #[clap(long, value_enum, default_value_t = ExportFormat::Json)]
    pub format: ExportFormat,

    /// Use a specific configuration file
    #[clap(short = 'c', long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Increase verbosity (-v, -vv)
    #[clap(short, long, action = ArgAction::Count)]
    pub verbose: u8,

    /// Enable debug output
    #[clap(long)]
    pub debug: bool,

    /// Generate shell completions and exit
    #[clap(long, value_enum, value_name = "SHELL")]
    pub completions: Option<clap_complete::Shell>,

    /// Ebook path, history number, pattern, or URL
    #[clap(name = "EBOOK")]
    pub ebook: Vec<String>,
}
