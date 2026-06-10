//! wb300 — a live TUI control tower for Git worktrees used by parallel coding agents.
//!
//! This is the library crate behind the `wb300` binary; `src/main.rs` is a thin
//! wrapper around [`run`]. Keeping the logic in a library lets the parsers and
//! reducers (added phase by phase) be unit- and integration-tested directly.

pub mod agent;
pub mod app;
pub mod cleanup;
pub mod cli;
pub mod collision;
pub mod config;
pub mod git;
pub mod help;
pub mod home;
pub mod live;
pub mod notifications;
pub mod process;
pub mod storage;
pub mod terminal;
pub mod ui;
pub mod uninstall;
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
    // `wb300 agent`: a headless JSON snapshot for orchestrating agents.
    // Dispatched BEFORE the panic hook and any terminal setup, so nothing — not
    // even a panic's terminal-restore escape sequence — can contaminate the
    // pure-JSON stdout contract.
    if let Some(Command::Agent) = &cli.command {
        return run_agent(&cli).await;
    }

    // `wb300 help`: the full manual, no TUI.
    if let Some(Command::Help) = &cli.command {
        std::process::exit(help::run(!cli.no_color));
    }

    // `wb300 uninstall`: plain CLI flow, no TUI.
    if let Some(Command::Uninstall(args)) = &cli.command {
        std::process::exit(uninstall::run(args));
    }

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

    // `--home`/`--multi` forces the machine-wide view even inside a repo.
    if cli.home {
        return run_home_entry(&cli).await;
    }

    let repo = match git::RepoIdentity::discover(&start_dir).await {
        Ok(repo) => repo,
        Err(err) => {
            // An explicit `--repo` that isn't a repository is a user error worth
            // reporting; otherwise (relying on the cwd) open the home view.
            if cli.repo.is_some() {
                eprintln!("wb300: {err}");
                eprintln!("The path given with --repo is not inside a Git repository.");
                std::process::exit(2);
            }
            return run_home_entry(&cli).await;
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
    let result = run_loop(&mut terminal, snapshot, cli.no_live, effective_notify(&cli)).await;
    guard.restore();
    result
}

/// The notification config for this run: the user's config file, with the
/// master switch forced off by `--no-notify`.
fn effective_notify(cli: &Cli) -> config::NotifyConfig {
    let mut cfg = config::load().notifications;
    if cli.no_notify {
        cfg.enabled = false;
    }
    cfg
}

/// The render/input loop: a `tokio::select!` over terminal input, debounced
/// filesystem refreshes, a periodic poll backstop, an animation tick, and
/// completed snapshot captures. Captures run on spawned tasks so the UI never
/// blocks on Git (handoff §8).
async fn run_loop(
    terminal: &mut terminal::Tui,
    snapshot: git::RepoSnapshot,
    no_live: bool,
    notify_cfg: config::NotifyConfig,
) -> Result<()> {
    let repo = snapshot.repo.clone();
    let mut app = AppState::new(snapshot);

    // OS toast notifications: events flow reducer → loop → notifier task.
    // `try_send` everywhere — a dropped toast is fine, a blocked UI is not.
    let (notify_tx, notify_rx) = tokio::sync::mpsc::channel::<notifications::NotifyEvent>(64);
    let _notifier = notifications::spawn_notifier(notify_cfg, notify_rx);

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
    // Immediate (lossy) lane carrying changed paths for the live save marker,
    // split from the debounced refresh by the watcher's debouncer.
    let (activity_tx, mut activity_rx) = tokio::sync::mpsc::channel::<Vec<PathBuf>>(64);
    // Operator-facing result messages (e.g. kill outcomes) back to the reducer,
    // so a requested destructive action is always acknowledged on screen.
    let (status_tx, mut status_rx) = tokio::sync::mpsc::channel::<(String, bool)>(8);

    // Filesystem watcher → debouncer → refresh requests. The watcher is held for
    // its lifetime; if it can't start, the periodic poll still keeps us current.
    let mut live_status = if no_live {
        LiveStatus::Static
    } else {
        LiveStatus::PollOnly
    };
    let mut _watcher = None;
    if !no_live {
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<PathBuf>>();
        match live::fs_watcher::watch(&watch_paths(&app.snapshot), raw_tx) {
            Ok((watcher, watched)) => {
                _watcher = Some(watcher);
                live_status = LiveStatus::Live;
                tracing::info!("watching {watched} directories");
                tokio::spawn(live::debouncer::run(
                    raw_rx,
                    activity_tx.clone(),
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
            Some(paths) = activity_rx.recv() => app.note_activity(&paths),
            Some((text, err)) = status_rx.recv() => app.set_status(text, err),
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
                match result {
                    Some(snapshot) => {
                        app.set_stale(false);
                        let repo_label = home::repo_name(&snapshot);
                        for event in app.ingest_snapshot(snapshot) {
                            if let Some(n) = notifications::from_archived(&event, &repo_label) {
                                let _ = notify_tx.try_send(n);
                            }
                            let _ = event_writer.send(event);
                        }
                    }
                    // Capture failed: mark stale so the header stops claiming
                    // "live" while showing data that may now be wrong.
                    None => app.set_stale(true),
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

        // Pending process kill (confirmed by the operator) — off the UI task,
        // then re-capture so the now-gone agent disappears from the board.
        if let Some(kill) = app.take_pending_kill() {
            let tx = refresh_tx.clone();
            let stx = status_tx.clone();
            let pid = kill.pid;
            let short = kill.name.trim_end_matches(".exe").to_string();
            tokio::spawn(async move {
                let outcome =
                    tokio::task::spawn_blocking(move || process::kill(kill.pid, &kill.name)).await;
                let status = match outcome {
                    Ok(Ok(())) => {
                        tracing::info!("terminated process {pid} on request");
                        (format!("✓ terminated {short} (pid {pid})"), false)
                    }
                    Ok(Err(err)) => {
                        tracing::warn!("kill of process {pid} failed: {err}");
                        (format!("⚠ kill failed: {err}"), true)
                    }
                    Err(err) => {
                        tracing::warn!("kill task for process {pid} panicked: {err}");
                        ("⚠ kill failed: internal error".to_string(), true)
                    }
                };
                // Surface the outcome on screen, then re-capture so a killed
                // process leaves the board.
                let _ = stx.send(status).await;
                let _ = tx.send(()).await;
            });
        }

        // Coalesce concurrent triggers: one capture at a time, off the UI task.
        if want_refresh && !in_flight {
            in_flight = true;
            let tx = snap_tx.clone();
            let repo = repo.clone();
            tokio::spawn(async move {
                // Log the failure (was silently `.ok()`-dropped) so a wedged Git
                // subprocess is diagnosable; the `None` drives the stale flag.
                let result = match git::RepoSnapshot::capture(repo).await {
                    Ok(snapshot) => Some(snapshot),
                    Err(err) => {
                        tracing::warn!("snapshot capture failed (showing stale state): {err}");
                        None
                    }
                };
                let _ = tx.send(result).await;
            });
        }
    }
    Ok(())
}

/// Best-effort: route `tracing` diagnostics to `<state_dir>/wb300.log` so
/// warnings (watcher/archive failures) are observable without corrupting the TUI.
fn init_logging(repo: &git::RepoIdentity) {
    install_file_logging(&repo.state_dir());
}

/// Install a file-backed `tracing` subscriber writing to `<dir>/wb300.log`.
/// Idempotent (`try_init`) and never fatal — logging is a convenience, not a
/// requirement. Shared by the per-repo and home entry points.
fn install_file_logging(dir: &std::path::Path) {
    if std::fs::create_dir_all(dir).is_err() {
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

/// `wb300 agent`: capture the current repo (or the machine-wide view) and print
/// it as JSON, no TUI. The schema is stable (`wb300.agent.v1`, see `agent.rs`).
async fn run_agent(cli: &Cli) -> Result<()> {
    let report = if cli.home {
        agent_home_report().await
    } else {
        let start_dir = match &cli.repo {
            Some(path) => path.clone(),
            None => std::env::current_dir()?,
        };
        match git::RepoIdentity::discover(&start_dir).await {
            Ok(repo) => {
                let snapshot = git::RepoSnapshot::capture(repo).await?;
                agent::Report::repo(&snapshot)
            }
            Err(err) => {
                // An explicit --repo that isn't a repository is an error;
                // otherwise fall back to the machine-wide JSON.
                if cli.repo.is_some() {
                    eprintln!("wb300: {err}");
                    std::process::exit(2);
                }
                agent_home_report().await
            }
        }
    };
    println!("{}", report.to_json());
    Ok(())
}

/// Capture the machine-wide view and shape it into an agent [`agent::Report`].
async fn agent_home_report() -> agent::Report {
    let config = home::HomeConfig::from_env();
    let snapshot = home::HomeSnapshot::capture(&config).await;
    agent::Report::home(&snapshot.repos, snapshot.scanned_at)
}

/// Entry point for the machine-wide home view: capture every active repo, then
/// run the home event loop (or print the first repo root for the `cd` contract).
async fn run_home_entry(cli: &Cli) -> Result<()> {
    let config = home::HomeConfig::from_env();
    install_file_logging(&home::home_state_dir());

    let snapshot = home::HomeSnapshot::capture(&config).await;

    if cli.print_selected_path {
        // No single selection in the home view; emit the first repo root (or
        // nothing) so the shell `cd` integration degrades gracefully.
        if let Some(repo) = snapshot.repos.first() {
            println!("{}", repo.repo.root.display());
        }
        return Ok(());
    }

    let options = terminal::TerminalOptions {
        alt_screen: !cli.no_alt_screen,
        mouse: false,
    };
    let (mut guard, mut terminal) = terminal::TerminalGuard::enter(options)?;
    let result = run_home_loop(
        &mut terminal,
        config,
        snapshot,
        cli.no_live,
        effective_notify(cli),
    )
    .await;
    guard.restore();
    result
}

/// The home-view render/input loop. Mirrors [`run_loop`] at a coarser grain: a
/// `tokio::select!` over input, a filesystem-watch refresh, a periodic rescan,
/// an animation tick, and completed machine-wide captures (which run off the UI
/// task). `Enter` hands control to the per-repo [`run_loop`] for the selection,
/// then returns here and rescans.
async fn run_home_loop(
    terminal: &mut terminal::Tui,
    config: home::HomeConfig,
    snapshot: home::HomeSnapshot,
    no_live: bool,
    notify_cfg: config::NotifyConfig,
) -> Result<()> {
    let config = std::sync::Arc::new(config);
    let mut home = home::HomeState::new(snapshot);

    // Machine-wide toasts: every matched repo's milestones flow through here.
    let (notify_tx, notify_rx) = tokio::sync::mpsc::channel::<notifications::NotifyEvent>(64);
    let _notifier = notifications::spawn_notifier(notify_cfg.clone(), notify_rx);

    let (snap_tx, mut snap_rx) = tokio::sync::mpsc::channel::<home::HomeSnapshot>(4);
    let (refresh_tx, mut refresh_rx) = tokio::sync::mpsc::channel::<()>(8);
    let (activity_tx, mut activity_rx) = tokio::sync::mpsc::channel::<Vec<PathBuf>>(64);

    // Filesystem watcher across every discovered worktree root (bounded by the
    // same global cap as the per-repo watcher). The periodic rescan is the
    // correctness backstop and also picks up newly-started/stopped agents.
    let mut live_status = if no_live {
        LiveStatus::Static
    } else {
        LiveStatus::PollOnly
    };
    let mut _watcher = None;
    if !no_live {
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<PathBuf>>();
        match live::fs_watcher::watch(&home.snapshot.watch_paths(), raw_tx) {
            Ok((watcher, watched)) => {
                _watcher = Some(watcher);
                live_status = LiveStatus::Live;
                tracing::info!("home: watching {watched} directories");
                tokio::spawn(live::debouncer::run(
                    raw_rx,
                    activity_tx.clone(),
                    refresh_tx.clone(),
                    Duration::from_millis(400),
                ));
            }
            Err(err) => tracing::warn!("home watcher unavailable: {err}"),
        }
    }
    home.set_live_status(live_status);

    let mut poll = tokio::time::interval(Duration::from_millis(2500));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut anim = tokio::time::interval(Duration::from_millis(250));
    let mut events = EventStream::new();
    let mut in_flight = false;

    loop {
        terminal.draw(|frame| ui::home::render(frame, &home))?;
        if home.should_quit {
            break;
        }

        let mut want_refresh = false;
        tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => home.handle_key(key),
                Some(Ok(_)) => {}
                Some(Err(err)) => return Err(err.into()),
                None => break,
            },
            Some(()) = refresh_rx.recv() => want_refresh = true,
            Some(paths) = activity_rx.recv() => home.note_activity(&paths),
            _ = poll.tick(), if !no_live => want_refresh = true,
            _ = anim.tick() => home.expire_transitions(),
            Some(snapshot) = snap_rx.recv() => {
                in_flight = false;
                for event in home.ingest_snapshot(snapshot) {
                    // Home events arrive repo-stamped; no fallback label needed.
                    if let Some(n) = notifications::from_archived(&event, "") {
                        let _ = notify_tx.try_send(n);
                    }
                }
            }
        }

        // Drill-in: hand the terminal to the per-repo view for the selected
        // repo, then return to the home view and rescan to pick up any change.
        if let Some(repo_snapshot) = home.take_drill_in() {
            run_loop(terminal, repo_snapshot, no_live, notify_cfg.clone()).await?;
            // While drilled in, the home `select!` wasn't draining `refresh_rx`,
            // so the fs-watch debouncer may have queued a backlog. Drain it (and
            // unblock the debouncer) so we do one fresh rescan, not a burst.
            while refresh_rx.try_recv().is_ok() {}
            // Likewise drop any queued activity batches so we don't replay stale
            // save-pulses for events that happened during the drill-in.
            while activity_rx.try_recv().is_ok() {}
            want_refresh = true;
        }

        if home.take_pending_refresh() {
            want_refresh = true;
        }

        // One rescan at a time, off the UI task.
        if want_refresh && !in_flight {
            in_flight = true;
            let tx = snap_tx.clone();
            let config = config.clone();
            tokio::spawn(async move {
                let snapshot = home::HomeSnapshot::capture(&config).await;
                let _ = tx.send(snapshot).await;
            });
        }
    }
    Ok(())
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
