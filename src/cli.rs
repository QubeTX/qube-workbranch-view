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
    about = "Live TUI control tower for Git worktrees used by parallel coding agents"
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
    // Self-update wb300 to the latest release.
    Update(UpdateArgs),
    // Print the full repository state as JSON and exit (no TUI) — a headless
    // snapshot for orchestrating agents. Outside a repo (or with --home) it
    // prints the machine-wide view of every active repository.
    Agent,
}

#[derive(Args, Debug)]
pub struct UpdateArgs {
    // Emit machine-readable JSON describing the update outcome.
    #[arg(long)]
    pub json: bool,
}
