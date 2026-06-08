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
    #[arg(long, value_name = "PATH")]
    pub repo: Option<PathBuf>,

    // Open in static snapshot mode (no live filesystem/process/remote engine).
    #[arg(long)]
    pub no_live: bool,

    // Do not use the alternate screen (fallback / debug renderer).
    #[arg(long)]
    pub no_alt_screen: bool,

    // Disable colored output.
    #[arg(long)]
    pub no_color: bool,

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
}

#[derive(Args, Debug)]
pub struct UpdateArgs {
    // Emit machine-readable JSON describing the update outcome.
    #[arg(long)]
    pub json: bool,
}
