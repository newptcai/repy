use clap::{ArgAction, Parser};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(
    name = "repy",
    version,
    about = "A Rust port of the awesome CLI Ebook Reader `epy` by `wustho`.",
    long_about = None
)]
pub struct Cli {
    /// Print reading history
    #[clap(short = 'r', long)]
    pub history: bool,

    /// Dump the content of the ebook
    #[clap(short, long)]
    pub dump: bool,

    /// Use a specific configuration file
    #[clap(short = 'c', long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Increase verbosity (-v, -vv)
    #[clap(short, long, action = ArgAction::Count)]
    pub verbose: u8,

    /// Enable debug output
    #[clap(long)]
    pub debug: bool,

    /// Ebook path, history number, pattern, or URL
    #[clap(name = "EBOOK")]
    pub ebook: Vec<String>,
}
