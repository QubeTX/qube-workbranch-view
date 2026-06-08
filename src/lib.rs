//! wb300 — a live TUI control tower for Git worktrees used by parallel coding agents.
//!
//! This is the library crate behind the `wb300` binary; `src/main.rs` is a thin
//! wrapper around [`run`]. Keeping the logic in a library lets the parsers and
//! reducers (added phase by phase) be unit- and integration-tested directly.

pub mod app;
pub mod cli;
pub mod terminal;
pub mod ui;

use app::AppState;
use cli::{Cli, Command};
use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;

/// Run wb300: dispatch any subcommand, otherwise launch the live TUI.
pub async fn run(cli: Cli) -> Result<()> {
    terminal::install_panic_hook()?;

    if let Some(Command::Update(_)) = cli.command {
        // Registry-aware self-update lands in the packaging phase (Phase 9).
        println!("wb300 update is not implemented yet.");
        return Ok(());
    }

    let repo_label = repo_label(&cli);

    if cli.print_selected_path {
        // Shell `cd` integration is wired once worktree selection exists; until
        // then, emit the starting directory so the contract is stable.
        println!("{repo_label}");
        return Ok(());
    }

    let options = terminal::TerminalOptions {
        alt_screen: !cli.no_alt_screen,
        mouse: false,
    };
    let (mut guard, mut terminal) = terminal::TerminalGuard::enter(options)?;
    let result = run_loop(&mut terminal, repo_label).await;
    guard.restore();
    result
}

/// Best-effort human-readable label for the repository under inspection.
fn repo_label(cli: &Cli) -> String {
    if let Some(repo) = &cli.repo {
        return repo.display().to_string();
    }
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

/// The render/input loop. Pure event-driven for now; the live engine will add
/// ticks via a `tokio::select!` here in Phase 4.
async fn run_loop(terminal: &mut terminal::Tui, repo_label: String) -> Result<()> {
    let mut app = AppState::new(repo_label);
    let mut events = EventStream::new();

    while !app.should_quit {
        terminal.draw(|frame| ui::render(frame, &app))?;

        match events.next().await {
            // Filter to key *presses*: Windows terminals also emit Release/Repeat.
            Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                let action = app.resolve_key(key);
                app.apply(action);
            }
            // Resize / mouse / paste — the next loop iteration redraws.
            Some(Ok(_)) => {}
            Some(Err(err)) => return Err(err.into()),
            // Event stream closed (e.g. stdin EOF): exit cleanly.
            None => break,
        }
    }
    Ok(())
}
