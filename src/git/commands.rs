//! Async wrapper around the installed `git` binary.
//!
//! The TUI must never block on Git, so every call runs on Tokio with a timeout.
//! We use the user's own `git` so their config, credential helpers, SSH setup,
//! and Git-for-Windows behavior all apply unchanged.

use std::borrow::Cow;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use tokio::process::Command;

/// Default per-command timeout. Local Git operations are fast; a hang almost
/// always means a credential/network prompt we've already suppressed.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// The result of running `git`.
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl GitOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    pub fn stdout_str(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    pub fn stderr_str(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }
}

/// Run `git <args>` in `cwd` (or the process cwd) with the default timeout.
pub async fn run_git(cwd: Option<&Path>, args: &[&str]) -> Result<GitOutput> {
    run_git_timeout(cwd, args, DEFAULT_TIMEOUT).await
}

/// Run `git <args>` with an explicit timeout.
pub async fn run_git_timeout(
    cwd: Option<&Path>,
    args: &[&str],
    timeout: Duration,
) -> Result<GitOutput> {
    let mut cmd = Command::new("git");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Never let git block the UI waiting on an interactive credential prompt.
        .env("GIT_TERMINAL_PROMPT", "0");

    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| eyre!("git {args:?} timed out after {timeout:?}"))??;

    Ok(GitOutput {
        code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

/// Run `git <args>` and require success, returning stdout bytes (NUL-safe).
pub async fn git_stdout(cwd: Option<&Path>, args: &[&str]) -> Result<Vec<u8>> {
    let out = run_git(cwd, args).await?;
    if !out.success() {
        return Err(eyre!(
            "git {args:?} failed ({}): {}",
            out.code
                .map_or_else(|| "signal".to_string(), |c| c.to_string()),
            out.stderr_str().trim()
        ));
    }
    Ok(out.stdout)
}
