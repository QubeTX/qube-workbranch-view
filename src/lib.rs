//! wb300 — a live TUI control tower for Git worktrees used by parallel coding agents.
//!
//! This is the library crate behind the `wb300` binary; `src/main.rs` is a thin
//! wrapper around [`run`]. Keeping the logic in a library lets the parsers and
//! reducers (added phase by phase) be unit- and integration-tested directly.

pub mod app;
pub mod cli;
pub mod git;
pub mod terminal;
pub mod ui;

use app::{Action, AppState};
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

    let start_dir = match &cli.repo {
        Some(path) => path.clone(),
        None => std::env::current_dir()?,
    };

    let repo = match git::RepoIdentity::discover(&start_dir).await {
        Ok(repo) => repo,
        Err(err) => {
            // Phase 10 will open the machine-wide home view here instead of exiting.
            eprintln!("wb300: {err}");
            eprintln!(
                "Run wb300 inside a Git repository. (The machine-wide view for non-repo \
                 directories arrives in a later phase.)"
            );
            std::process::exit(2);
        }
    };

    let snapshot = git::RepoSnapshot::capture(repo).await?;

    if cli.print_selected_path {
        // Until interactive selection lands, emit the repo root so the shell
        // `cd` integration contract stays stable.
        println!("{}", snapshot.repo.root.display());
        return Ok(());
    }

    let options = terminal::TerminalOptions {
        alt_screen: !cli.no_alt_screen,
        mouse: false,
    };
    let (mut guard, mut terminal) = terminal::TerminalGuard::enter(options)?;
    let result = run_loop(&mut terminal, snapshot).await;
    guard.restore();
    result
}

/// The render/input loop. Pure event-driven for now; the live engine will add
/// ticks via a `tokio::select!` here in Phase 4.
async fn run_loop(terminal: &mut terminal::Tui, snapshot: git::RepoSnapshot) -> Result<()> {
    let mut app = AppState::new(snapshot);
    let mut events = EventStream::new();

    while !app.should_quit {
        terminal.draw(|frame| ui::render(frame, &app))?;

        match events.next().await {
            // Filter to key *presses*: Windows terminals also emit Release/Repeat.
            Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                let action = app.resolve_key(key);
                if action == Action::Refresh {
                    // Re-capture off the reducer (it can't await). This briefly
                    // blocks input; Phase 4 will spawn it instead.
                    if let Ok(snapshot) =
                        git::RepoSnapshot::capture(app.snapshot.repo.clone()).await
                    {
                        app.set_snapshot(snapshot);
                    }
                } else {
                    app.apply(action);
                }
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
