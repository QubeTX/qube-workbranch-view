//! wb300 — a live TUI control tower for Git worktrees used by parallel coding agents.
//!
//! This is the library crate behind the `wb300` binary; `src/main.rs` is a thin
//! wrapper around [`run`]. Keeping the logic in a library lets the parsers and
//! reducers (added phase by phase) be unit- and integration-tested directly.

pub mod app;
pub mod cleanup;
pub mod cli;
pub mod collision;
pub mod git;
pub mod live;
pub mod process;
pub mod storage;
pub mod terminal;
pub mod ui;
pub mod update;
pub mod util;

use std::path::PathBuf;
use std::time::Duration;

/// The crate version (`CARGO_PKG_VERSION`), surfaced for `wb300 update`'s
/// "current version" comparison and the `--version` flag.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use app::{AppState, LiveStatus};
use cli::{Cli, Command};
use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;

/// Keep archived events for 30 days, capped to this many (handoff §18 "archive").
const ARCHIVE_KEEP_SECS: u64 = 30 * 86_400;
const ARCHIVE_MAX: usize = 2000;

/// Run wb300: dispatch any subcommand, otherwise launch the live TUI.
pub async fn run(cli: Cli) -> Result<()> {
    terminal::install_panic_hook()?;

    if let Some(Command::Update(args)) = &cli.command {
        // Self-update runs entirely outside the TUI (no terminal takeover) and
        // returns a process exit code; `--no-color` drives both ANSI color and
        // the ASCII-vs-Unicode glyph choice.
        let opts = update::UpdateOptions {
            json: args.json,
            use_colors: !cli.no_color,
            use_unicode: !cli.no_color,
        };
        std::process::exit(update::run(&opts));
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

    init_logging(&repo);

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

    // Load + prune (age + cap) the persisted Timeline, then keep one writer
    // thread that owns the file for ordered, non-interleaved appends.
    let events_path = app.snapshot.repo.state_dir().join("events.jsonl");
    app.set_events(storage::event_store::prune(
        &events_path,
        ARCHIVE_KEEP_SECS,
        ARCHIVE_MAX,
    ));
    let event_writer = storage::event_store::spawn_writer(events_path);

    // The capture channel carries Option: None means a capture failed, so the
    // loop still resets `in_flight` and never wedges the live engine.
    let (snap_tx, mut snap_rx) = tokio::sync::mpsc::channel::<Option<git::RepoSnapshot>>(8);
    let (refresh_tx, mut refresh_rx) = tokio::sync::mpsc::channel::<()>(8);
    let (fetch_done_tx, mut fetch_done_rx) = tokio::sync::mpsc::channel::<bool>(4);

    // Filesystem watcher → debouncer → refresh requests. The watcher is held for
    // its lifetime; if it can't start, the periodic poll still keeps us current.
    let mut live_status = if no_live {
        LiveStatus::Static
    } else {
        LiveStatus::PollOnly
    };
    let mut _watcher = None;
    if !no_live {
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        match live::fs_watcher::watch(&watch_paths(&app.snapshot), raw_tx) {
            Ok((watcher, watched)) => {
                _watcher = Some(watcher);
                live_status = LiveStatus::Live;
                tracing::info!("watching {watched} directories");
                tokio::spawn(live::debouncer::run(
                    raw_rx,
                    refresh_tx.clone(),
                    Duration::from_millis(300),
                ));
            }
            Err(err) => tracing::warn!("filesystem watcher unavailable: {err}"),
        }
    }
    app.set_live_status(live_status);

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
                    app.apply(action);
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => return Err(err.into()),
                None => break,
            },
            Some(()) = refresh_rx.recv() => want_refresh = true,
            _ = poll.tick(), if !no_live => want_refresh = true,
            _ = anim.tick() => app.expire_transitions(),
            Some(ok) = fetch_done_rx.recv() => {
                app.set_fetching(false);
                if ok {
                    app.set_remote_checked(storage::events::epoch_secs());
                }
                want_refresh = true;
            }
            Some(result) = snap_rx.recv() => {
                in_flight = false;
                if let Some(snapshot) = result {
                    for event in app.ingest_snapshot(snapshot) {
                        let _ = event_writer.send(event);
                    }
                }
            }
        }

        // `r` flows through the reducer (apply → pending flag); consume it here.
        if app.take_pending_refresh() {
            want_refresh = true;
        }
        // `f` flows through the reducer; the fetch runs off the UI task and a
        // re-capture follows on completion to pick up new ahead/behind state.
        // Guard before consuming the flag so a re-press during a fetch isn't lost.
        // (Like the capture task, a panic before the completion send would strand
        // `fetching`; run_git returns Result and never panics, so we accept that
        // rather than juggling a JoinHandle.)
        if !app.fetching && app.take_pending_fetch() {
            app.set_fetching(true);
            let tx = fetch_done_tx.clone();
            let root = repo.root.clone();
            tokio::spawn(async move {
                let ok = git::commands::run_git(Some(&root), &["fetch", "--all", "--prune"])
                    .await
                    .map(|out| out.success())
                    .unwrap_or(false);
                let _ = tx.send(ok).await;
            });
        }

        // Pending Git mutation (remove / prune) from a confirm dialog or palette.
        if let Some(pending) = app.take_pending_git() {
            let tx = refresh_tx.clone();
            let repo = repo.clone();
            tokio::spawn(async move {
                run_pending_git(&repo, pending).await;
                // Trigger a re-capture so the change shows immediately.
                let _ = tx.send(()).await;
            });
        }

        // Coalesce concurrent triggers: one capture at a time, off the UI task.
        if want_refresh && !in_flight {
            in_flight = true;
            let tx = snap_tx.clone();
            let repo = repo.clone();
            tokio::spawn(async move {
                let result = git::RepoSnapshot::capture(repo).await.ok();
                let _ = tx.send(result).await;
            });
        }
    }
    Ok(())
}

/// Best-effort: route `tracing` diagnostics to `<state_dir>/wb300.log` so
/// warnings (watcher/archive failures) are observable without corrupting the TUI.
fn init_logging(repo: &git::RepoIdentity) {
    let dir = repo.state_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("wb300.log"))
    else {
        return;
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("wb300=warn"));
    let _ = tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init();
}

/// Run a confirmed Git mutation off the UI task: a rescue snapshot for dirty
/// removals, then remove / prune. Failures are logged, never panicked.
async fn run_pending_git(repo: &git::RepoIdentity, pending: app::PendingGit) {
    match pending {
        app::PendingGit::RemoveWorktree {
            path,
            force,
            snapshot_first,
            label,
        } => {
            if snapshot_first {
                let dest = repo.state_dir().join("snapshots");
                if let Err(err) = git::ops::snapshot_worktree(
                    std::path::Path::new(&path),
                    &dest,
                    &label,
                    storage::events::epoch_secs(),
                )
                .await
                {
                    // Never force-remove dirty work we couldn't rescue.
                    tracing::error!("rescue snapshot failed — aborting removal: {err}");
                    return;
                }
            }
            if let Err(err) = git::ops::remove_worktree(&repo.root, &path, force).await {
                tracing::warn!("worktree remove failed: {err}");
            }
        }
        app::PendingGit::Prune => {
            if let Err(err) = git::ops::prune_worktrees(&repo.root).await {
                tracing::warn!("worktree prune failed: {err}");
            }
        }
    }
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
