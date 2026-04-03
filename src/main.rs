mod action;
mod app;
mod clipboard;
mod command;
mod config;
mod event;
mod highlight;
mod http_client;
mod keybinding_config;
mod keybindings;
mod model;
mod parser;
mod state;
mod theme;
mod tui;
mod ui;
mod vim_buffer;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "restui", version, about = "A TUI HTTP client")]
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

    /// Inherit colors from Neovim (passed by restui.nvim plugin)
    /// Format: "bg=#1e1e2e,fg=#cdd6f4,accent=#89b4fa,..."
    #[arg(long)]
    colors: Option<String>,

    /// Enable debug logging to ~/.local/share/restui/restui.log
    #[arg(long)]
    debug: bool,

    /// Print default keybindings config to stdout and exit
    #[arg(long)]
    dump_keybindings: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Panic hook: always restore terminal even on crash
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = tui::restore();
        default_panic(info);
    }));

    let cli = Cli::parse();

    if cli.dump_keybindings {
        print!("{}", keybinding_config::generate_default_toml());
        return Ok(());
    }

    // Set up file-based logging (only when --debug is passed)
    let _log_guard = if cli.debug {
        let log_dir = config::data_dir();
        let _ = std::fs::create_dir_all(&log_dir);
        let file_appender = tracing_appender::rolling::never(&log_dir, "restui.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("restui=debug")),
            )
            .with_writer(non_blocking)
            .with_ansi(false)
            .init();
        tracing::info!("restui v{} started with --debug", env!("CARGO_PKG_VERSION"));
        Some(guard)
    } else {
        None
    };

    let config = config::AppConfig::load().unwrap_or_default();
    let kb_toml = match keybinding_config::load_keybindings_toml() {
        Ok(toml) => toml,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let keybindings = keybinding_config::build_config(kb_toml);
    let mut app = app::App::new(config.clone(), keybindings);

    let mut dirs = config.general.http_file_dirs.clone();
    if let Some(dir) = cli.dir {
        dirs = vec![dir];
    }
    if let Some(ref file) = cli.file {
        if let Some(parent) = file.parent() {
            dirs.push(parent.to_path_buf());
        }
    }

    // Apply nvim colorscheme if --colors was passed
    if let Some(ref colors) = cli.colors {
        app.state.theme = theme::Theme::from_nvim_colors(colors);
    }

    app.load_collections(&dirs);
    app.load_environments(cli.env_file.as_deref());

    if let Some(ref file) = cli.file {
        if let Some(file_stem) = file.file_stem().and_then(|s| s.to_str()) {
            for (i, collection) in app.state.collections.iter().enumerate() {
                if collection.name == file_stem {
                    app.state.collections_view.list_state.select(Some(i));
                    if let Some(req) = collection.requests.first() {
                        app.state.current_request = req.clone();
                    }
                    break;
                }
            }
        }
    }

    let mut terminal = tui::init()?;
    let result = app.run(&mut terminal).await;
    tui::restore()?;

    result
}
