mod cli;
mod config;
mod ebook;
mod models;
mod settings;
mod state;

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use eyre::Result;
use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{io, time::{Duration, Instant}};

use crate::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    println!("{:?}", cli);

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let res = run_app(&mut terminal);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("{:?}", err);
    }

    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    Ok(())
}
