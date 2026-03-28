mod action;
mod app;
mod clipboard;
mod config;
mod event;
mod highlight;
mod http_client;
mod keybindings;
mod model;
mod parser;
mod state;
mod tui;
mod ui;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "restui", version, about = "A lazygit-style TUI HTTP client")]
struct Cli {
    /// Path to a .http or .yaml file to open
    #[arg(short, long)]
    file: Option<PathBuf>,

    /// Path to environment file (env.json, env.yaml)
    #[arg(short = 'E', long)]
    env_file: Option<String>,

    /// Working directory to scan for .http files
    #[arg(short, long)]
    dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = config::AppConfig::load().unwrap_or_default();

    // Initialize app
    let mut app = app::App::new(config.clone());

    // Determine directories to scan
    let mut dirs = config.general.http_file_dirs.clone();
    if let Some(dir) = cli.dir {
        dirs = vec![dir];
    }
    if let Some(ref file) = cli.file {
        if let Some(parent) = file.parent() {
            dirs.push(parent.to_path_buf());
        }
    }

    // Load data
    app.load_collections(&dirs);
    app.load_environments(cli.env_file.as_deref());

    // If a specific file was provided, try to select it
    if let Some(ref file) = cli.file {
        if let Some(file_stem) = file.file_stem().and_then(|s| s.to_str()) {
            for (i, collection) in app.state.collections.iter().enumerate() {
                if collection.name == file_stem {
                    app.state.collections_state.select(Some(i));
                    if let Some(req) = collection.requests.first() {
                        app.state.current_request = req.clone();
                    }
                    break;
                }
            }
        }
    }

    // Initialize terminal
    let mut terminal = tui::init()?;

    // Run app
    let result = app.run(&mut terminal).await;

    // Restore terminal
    tui::restore()?;

    result
}
