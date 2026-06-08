//! wb300 — a live TUI control tower for Git worktrees used by parallel coding agents.
//!
//! This is the library crate behind the `wb300` binary; `src/main.rs` is a thin
//! wrapper around [`run`]. Keeping the logic in a library lets the parsers and
//! reducers (added phase by phase) be unit- and integration-tested directly.

pub mod app;
pub mod cli;
pub mod git;
pub mod live;
pub mod process;
pub mod terminal;
pub mod ui;
pub mod util;

use std::path::PathBuf;
use std::time::Duration;

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
    let result = run_loop(&mut terminal, snapshot, cli.no_live).await;
    guard.restore();
    result
}

/// The render/input loop: a `tokio::select!` over terminal input, debounced
/// filesystem refreshes, a periodic poll backstop, an animation tick, and
/// completed snapshot captures. Captures run on spawned tasks so the UI never
/// blocks on Git (handoff §8).
async fn run_loop(
    terminal: &mut terminal::Tui,
    snapshot: git::RepoSnapshot,
    no_live: bool,
) -> Result<()> {
    let repo = snapshot.repo.clone();
    let mut app = AppState::new(snapshot);

    let (snap_tx, mut snap_rx) = tokio::sync::mpsc::channel::<git::RepoSnapshot>(8);
    let (refresh_tx, mut refresh_rx) = tokio::sync::mpsc::channel::<()>(8);

    // Filesystem watcher → debouncer → refresh requests. The watcher is held for
    // its lifetime; if it can't start, the periodic poll still keeps us current.
    let mut _watcher = None;
    if !no_live {
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        match live::fs_watcher::watch(&watch_paths(&app.snapshot), raw_tx) {
            Ok(watcher) => {
                _watcher = Some(watcher);
                tokio::spawn(live::debouncer::run(
                    raw_rx,
                    refresh_tx.clone(),
                    Duration::from_millis(300),
                ));
            }
            Err(err) => tracing::warn!("filesystem watcher unavailable: {err}"),
        }
    }

    let mut poll = tokio::time::interval(Duration::from_millis(1500));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut anim = tokio::time::interval(Duration::from_millis(250));
    let mut events = EventStream::new();
    let mut in_flight = false;

    loop {
        terminal.draw(|frame| ui::render(frame, &app))?;
        if app.should_quit {
            break;
        }

        let mut want_refresh = false;
        tokio::select! {
            // Filter to key *presses*: Windows terminals also emit Release/Repeat.
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    let action = app.resolve_key(key);
                    if action == Action::Refresh {
                        want_refresh = true;
                    } else {
                        app.apply(action);
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => return Err(err.into()),
                None => break,
            },
            Some(()) = refresh_rx.recv() => want_refresh = true,
            _ = poll.tick(), if !no_live => want_refresh = true,
            _ = anim.tick() => app.expire_transitions(),
            Some(snapshot) = snap_rx.recv() => {
                in_flight = false;
                app.ingest_snapshot(snapshot);
            }
        }

        // Coalesce concurrent triggers: one capture at a time, off the UI task.
        if want_refresh && !in_flight {
            in_flight = true;
            let tx = snap_tx.clone();
            let repo = repo.clone();
            tokio::spawn(async move {
                if let Ok(snapshot) = git::RepoSnapshot::capture(repo).await {
                    let _ = tx.send(snapshot).await;
                }
            });
        }
    }
    Ok(())
}

/// Paths the filesystem watcher should watch: the repo root and every linked
/// worktree root (the common git dir lives under the main root, so it's covered).
fn watch_paths(snapshot: &git::RepoSnapshot) -> Vec<PathBuf> {
    let mut paths = vec![snapshot.repo.root.clone()];
    for wt in &snapshot.worktrees {
        if !wt.bare {
            paths.push(PathBuf::from(&wt.path));
        }
    }
    paths.sort();
    paths.dedup();
    paths
}
