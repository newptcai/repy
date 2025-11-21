use clap::Parser;

#[derive(Parser, Debug)]
#[clap(
    name = "repy",
    version,
    about = "A Rust port of the awesome CLI Ebook Reader `epy` by `wustho`.",
    long_about = None
)]
pub struct Cli {
    /// Print reading history
    #[clap(short, long)]
    pub history: bool,

    /// Dump the content of the ebook
    #[clap(short, long)]
    pub dump: bool,

    /// Ebook path, history number, pattern, or URL
    #[clap(name = "EBOOK")]
    pub ebook: Vec<String>,
}
