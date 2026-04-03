use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use devpulse::app::{App, AppMode};
use devpulse::config::Config;
use devpulse::ui;

#[derive(Parser, Debug)]
#[command(
    name = "devpulse",
    version,
    about = "Unified Developer Environment TUI"
)]
pub struct Cli {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(short, long)]
    pub filter: Option<String>,
    #[arg(short, long)]
    pub layout: Option<String>,
    #[arg(long)]
    pub no_docker: bool,
    #[arg(long)]
    pub refresh: Option<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load config
    let config_path = cli.config.unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("devpulse")
            .join("config.toml")
    });
    let mut config = Config::load(&config_path)?;

    // Apply CLI overrides
    if let Some(refresh) = cli.refresh {
        config.general.refresh_rate_ms = refresh.clamp(1000, 30000);
    }

    let tick_rate = Duration::from_millis(config.general.refresh_rate_ms);

    // Create app
    let mut app = App::new(config);

    // Apply initial filter from CLI
    if let Some(filter) = cli.filter {
        app.global_filter.set_query(&filter);
    }

    if cli.no_docker {
        app.docker_available = false;
    }

    // Initialize terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Initial data fetch
    app.tick();
    app.tick_docker().await;

    // Event loop
    let mut last_tick = Instant::now();
    loop {
        // Draw
        terminal.draw(|f| ui::draw(f, &app))?;

        // Poll events
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let was_confirm = matches!(app.mode, AppMode::Confirm);
                    ui::handle_key(&mut app, key);

                    // If we just confirmed an action, execute it
                    if was_confirm
                        && matches!(app.mode, AppMode::Normal)
                        && app.pending_action.is_some()
                    {
                        let action = app.pending_action.take().unwrap();
                        app.execute_action(&action).await;
                        // Refresh data after action
                        app.tick();
                        app.tick_docker().await;
                    }
                }
            }
        }

        // Tick
        if last_tick.elapsed() >= tick_rate {
            app.tick();
            app.tick_docker().await;
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_default_cli_args() {
        let args = Cli::parse_from(["devpulse"]);
        assert!(args.config.is_none());
        assert!(args.filter.is_none());
        assert_eq!(args.layout, None);
        assert!(!args.no_docker);
        assert!(args.refresh.is_none());
    }

    #[test]
    fn test_cli_args_with_options() {
        let args = Cli::parse_from([
            "devpulse",
            "--config",
            "/tmp/config.toml",
            "--filter",
            "node",
            "--layout",
            "quad",
            "--no-docker",
            "--refresh",
            "5000",
        ]);
        assert_eq!(args.config.unwrap().to_str().unwrap(), "/tmp/config.toml");
        assert_eq!(args.filter.unwrap(), "node");
        assert_eq!(args.layout.unwrap(), "quad");
        assert!(args.no_docker);
        assert_eq!(args.refresh.unwrap(), 5000);
    }
}
