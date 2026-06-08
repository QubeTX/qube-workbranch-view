//! Terminal lifecycle: raw mode, the alternate screen, and panic-safe restore.
//!
//! Entering raw mode / the alternate screen mutates global terminal state; if
//! the process exits without undoing it the user is left with a broken shell.
//! [`TerminalGuard`]'s `Drop` impl and the hook from [`install_panic_hook`]
//! make restoration unconditional (handoff §20.1).

use std::io::{self, Stdout, Write};

use color_eyre::eyre::Result;
use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

/// The concrete Ratatui terminal type used throughout wb300.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// How the terminal should be set up.
#[derive(Debug, Clone, Copy)]
pub struct TerminalOptions {
    /// Use the alternate screen buffer (disabled by `--no-alt-screen`).
    pub alt_screen: bool,
    /// Capture mouse events.
    pub mouse: bool,
}

/// RAII guard that restores the terminal when dropped.
pub struct TerminalGuard {
    options: TerminalOptions,
    active: bool,
}

impl TerminalGuard {
    /// Enter raw mode + (optionally) the alternate screen and build the terminal.
    pub fn enter(options: TerminalOptions) -> Result<(Self, Tui)> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        if options.alt_screen {
            execute!(out, EnterAlternateScreen)?;
        }
        if options.mouse {
            execute!(out, EnableMouseCapture)?;
        }
        execute!(out, cursor::Hide)?;

        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        terminal.clear()?;
        Ok((
            Self {
                options,
                active: true,
            },
            terminal,
        ))
    }

    /// Restore the terminal. Idempotent; safe to call before `Drop` runs.
    pub fn restore(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let mut out = io::stdout();
        let _ = execute!(out, cursor::Show);
        if self.options.mouse {
            let _ = execute!(out, DisableMouseCapture);
        }
        if self.options.alt_screen {
            let _ = execute!(out, LeaveAlternateScreen);
        }
        let _ = disable_raw_mode();
        let _ = out.flush();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Best-effort restoration for the panic hook, independent of any live guard
/// (the guard may not be reachable from inside the hook).
fn force_restore() {
    let mut out = io::stdout();
    let _ = execute!(out, cursor::Show, DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = out.flush();
}

/// Install color-eyre's error + panic hooks, wrapped so the terminal is always
/// restored before any panic report is printed.
pub fn install_panic_hook() -> Result<()> {
    let (panic_hook, eyre_hook) = color_eyre::config::HookBuilder::default().into_hooks();
    eyre_hook.install()?;
    let panic_hook = panic_hook.into_panic_hook();
    std::panic::set_hook(Box::new(move |info| {
        force_restore();
        panic_hook(info);
    }));
    Ok(())
}
