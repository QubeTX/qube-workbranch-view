// Command-line interface for wb300.
//
// IMPORTANT: build.rs `include!`s this file to generate the man page, so it must
// use line comments (`//`) only — never `///` or `//!` doc comments — and must
// not reference anything outside `clap` and `std`.
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "wb300",
    version,
    about = "Live TUI control tower for the branches and worktrees used by parallel coding agents",
    // `wb300 help` is our full manual (src/help.rs), not clap's usage dump.
    disable_help_subcommand = true
)]
pub struct Cli {
    // Path to the Git repository to open (defaults to the current directory).
    // Global so it is also accepted after a subcommand (e.g. `agent --repo X`).
    #[arg(long, value_name = "PATH", global = true)]
    pub repo: Option<PathBuf>,

    // Open in static snapshot mode (no live filesystem/process/remote engine).
    #[arg(long)]
    pub no_live: bool,

    // Force the machine-wide home view even when inside a Git repository.
    // (Without this, the home view opens automatically when run outside a repo.)
    // Global so it is also accepted after a subcommand (e.g. `agent --home`).
    #[arg(long, visible_alias = "multi", global = true)]
    pub home: bool,

    // Do not use the alternate screen (fallback / debug renderer).
    #[arg(long)]
    pub no_alt_screen: bool,

    // Disable colored output. Global so it applies to subcommands too.
    #[arg(long, global = true)]
    pub no_color: bool,

    // Disable OS toast notifications for this run (overrides the config file).
    #[arg(long)]
    pub no_notify: bool,

    // Print the selected worktree path on exit (for shell `cd` integration).
    #[arg(long)]
    pub print_selected_path: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    // Print the full manual — every view, key, glyph, and concept — and exit.
    Help,
    // Self-update wb300 to the latest release.
    Update(UpdateArgs),
    // Print the full repository state as JSON and exit (no TUI) — a headless
    // snapshot for orchestrating agents. Outside a repo (or with --home) it
    // prints the machine-wide view of every active repository.
    Agent,
    // Uninstall wb300 from this machine (detects how it was installed and
    // runs the matching uninstaller).
    Uninstall(UninstallArgs),
}

#[derive(Args, Debug)]
pub struct UpdateArgs {
    // Emit machine-readable JSON describing the update outcome.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct UninstallArgs {
    // Skip the confirmation prompt.
    #[arg(long, short = 'y')]
    pub yes: bool,

    // Also remove wb300's state, config, and registry entries (kept by default).
    #[arg(long)]
    pub purge: bool,

    // Emit machine-readable JSON describing the uninstall outcome.
    #[arg(long)]
    pub json: bool,
}
