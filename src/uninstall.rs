//! `wb300 uninstall` — remove wb300 from this machine, whatever installed it.
//!
//! Strategy per install channel:
//!
//! - **Windows MSI/EXE (the four first-class installers):** find wb300's
//!   Add/Remove Programs entry in the registry and launch its uninstaller
//!   (quiet variant when available, `/passive` for MSI), **detached**, then
//!   exit — Windows can't delete a running executable, so the uninstaller
//!   must outlive us.
//! - **Windows cargo / PowerShell installer:** schedule a detached
//!   `cargo uninstall wb300` that runs after this process exits (same
//!   running-exe constraint).
//! - **Unix:** `cargo uninstall wb300` when the binary lives in
//!   `~/.cargo/bin`, else unlink the binary directly (Unix allows deleting
//!   a running executable).
//!
//! `--purge` additionally removes the machine-wide state/config directories
//! and (Windows) the `HKCU\Software\WB300` + toast AppUserModelId registry
//! keys. Per-repo `.git/wb300/` event logs live inside your repositories and
//! are never touched. Uninstalling never deletes any repository data.

use crate::cli::UninstallArgs;

/// What happened, for both human and `--json` output.
struct Outcome {
    ok: bool,
    /// How the binary was installed (`msi-global`, `cargo-or-installer`, …).
    origin: String,
    /// What was done (or why nothing could be).
    action: String,
    purged: bool,
}

/// Run the uninstall flow. Returns the process exit code.
pub fn run(args: &UninstallArgs) -> i32 {
    if !args.yes && !confirm() {
        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "schema": "wb300.uninstall.v1",
                    "ok": false,
                    "action": "aborted by user",
                })
            );
        } else {
            println!("Aborted — nothing was removed.");
        }
        return 1;
    }

    let outcome = perform(args.purge);

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "schema": "wb300.uninstall.v1",
                "ok": outcome.ok,
                "origin": outcome.origin,
                "action": outcome.action,
                "purged": outcome.purged,
            })
        );
    } else {
        println!("{}", outcome.action);
        if outcome.purged {
            println!("State, config, and registry entries removed.");
        }
        if outcome.ok {
            println!("wb300 is uninstalling. Goodbye o7");
        }
    }
    if outcome.ok { 0 } else { 2 }
}

/// Type-to-confirm on stdin (skipped by `--yes`).
fn confirm() -> bool {
    use std::io::{BufRead, Write};
    print!("This removes the wb300 program from this machine.\nType \"uninstall\" to confirm: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    line.trim().eq_ignore_ascii_case("uninstall")
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn perform(purge: bool) -> Outcome {
    use crate::update::InstallOrigin;

    let origin = crate::update::detect_install_origin();
    let origin_id = origin.json_id().to_string();

    let (ok, action) = match origin {
        InstallOrigin::MsiGlobal
        | InstallOrigin::MsiCorporate
        | InstallOrigin::ExeGlobal
        | InstallOrigin::ExeCorporate => match find_uninstall_entry() {
            Some(entry) => match launch_uninstaller(&entry) {
                Ok(()) => (true, format!("Launched the uninstaller for {}.", entry.display_name)),
                Err(err) => (false, format!("Found the uninstaller but failed to launch it: {err}. Use Settings → Apps → Installed apps instead.")),
            },
            None => (
                false,
                "No Add/Remove Programs entry was found for wb300. Uninstall via Settings → Apps → Installed apps.".to_string(),
            ),
        },
        InstallOrigin::CargoOrInstaller => match schedule_cargo_uninstall() {
            Ok(()) => (
                true,
                "Scheduled `cargo uninstall wb300` (runs as soon as this process exits)."
                    .to_string(),
            ),
            Err(err) => (
                false,
                format!("Could not schedule cargo uninstall: {err}. Run `cargo uninstall wb300` from another terminal."),
            ),
        },
        InstallOrigin::Unknown => (
            false,
            "Could not determine how wb300 was installed (portable/custom location?). Delete the binary manually after this process exits.".to_string(),
        ),
    };

    let purged = purge && purge_windows();
    Outcome {
        ok,
        origin: origin_id,
        action,
        purged,
    }
}

/// One Add/Remove Programs entry.
#[cfg(windows)]
struct UninstallEntry {
    display_name: String,
    /// Prefer the quiet command when the installer recorded one (Inno).
    command: String,
    quiet: bool,
    is_msi: bool,
}

/// Scan the uninstall registry roots (per-machine 64-bit, per-machine 32-bit,
/// per-user) for wb300's entry.
#[cfg(windows)]
fn find_uninstall_entry() -> Option<UninstallEntry> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    const SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    const SUBKEY_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    let roots = [
        (RegKey::predef(HKEY_LOCAL_MACHINE), SUBKEY),
        (RegKey::predef(HKEY_LOCAL_MACHINE), SUBKEY_32),
        (RegKey::predef(HKEY_CURRENT_USER), SUBKEY),
    ];

    for (root, subkey) in roots {
        let Ok(uninstall) = root.open_subkey(subkey) else {
            continue;
        };
        for name in uninstall.enum_keys().flatten() {
            let Ok(entry) = uninstall.open_subkey(&name) else {
                continue;
            };
            let Ok(display): Result<String, _> = entry.get_value("DisplayName") else {
                continue;
            };
            // Exact product names only (per wix/*.wxs and inno/*.iss) — a
            // substring match could launch some other product's uninstaller.
            let lower = display.to_lowercase();
            if !(lower == "wb300" || lower == "wb300 (corporate edition)") {
                continue;
            }
            let quiet_cmd: Option<String> = entry.get_value("QuietUninstallString").ok();
            let loud_cmd: Option<String> = entry.get_value("UninstallString").ok();
            let (command, quiet) = match (quiet_cmd, loud_cmd) {
                (Some(q), _) if !q.trim().is_empty() => (q, true),
                (_, Some(l)) if !l.trim().is_empty() => (l, false),
                _ => continue,
            };
            let is_msi = command.to_lowercase().contains("msiexec");
            return Some(UninstallEntry {
                display_name: display,
                command,
                quiet,
                is_msi,
            });
        }
    }
    None
}

/// Launch the recorded uninstall command, detached, so it can remove this
/// (running) executable after we exit.
#[cfg(windows)]
fn launch_uninstaller(entry: &UninstallEntry) -> std::io::Result<()> {
    // Windows Installer records UninstallString as `MsiExec.exe /I{GUID}` for
    // many products (including our Global MSI) — /I on an installed product is
    // maintenance/REPAIR mode, not uninstall. Always rewrite to /X.
    let command = if entry.is_msi {
        msi_uninstall_command(&entry.command)
    } else {
        entry.command.clone()
    };
    let (program, args) = split_command_line(&command);
    let mut cmd = std::process::Command::new(program);
    for a in args {
        cmd.arg(a);
    }
    // MSI: we've already confirmed in the terminal — show progress only.
    if entry.is_msi && !entry.quiet {
        cmd.arg("/passive");
    }
    cmd.spawn().map(|_| ())
}

/// Force an msiexec command line into uninstall (`/X`) mode: registry
/// UninstallStrings use `/I{GUID}`, `/X{GUID}`, `/x {GUID}`, … — extract the
/// product GUID and rebuild as `MsiExec.exe /X{GUID}`. Falls back to the
/// original string when no GUID is present.
#[cfg(windows)]
fn msi_uninstall_command(command: &str) -> String {
    let (start, end) = match (command.find('{'), command.rfind('}')) {
        (Some(s), Some(e)) if e > s => (s, e),
        _ => return command.to_string(),
    };
    format!("MsiExec.exe /X{}", &command[start..=end])
}

/// Split a registry command line into (program, args): a leading quoted span
/// or the text up to the first space, then whitespace-separated arguments.
/// Known limit: a QUOTED argument after the program (e.g. `/LOG="a b.log"`)
/// would be shredded — none of the four shipped installers record one.
#[cfg(windows)]
fn split_command_line(command: &str) -> (String, Vec<String>) {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        match rest.split_once('"') {
            Some((program, tail)) => (
                program.to_string(),
                tail.split_whitespace().map(str::to_string).collect(),
            ),
            None => (rest.to_string(), Vec::new()),
        }
    } else {
        match command.split_once(' ') {
            Some((program, tail)) => (
                program.to_string(),
                tail.split_whitespace().map(str::to_string).collect(),
            ),
            None => (command.to_string(), Vec::new()),
        }
    }
}

/// Windows can't delete a running exe, so run cargo uninstall from a detached
/// helper that waits ~2s for this process to exit first.
#[cfg(windows)]
fn schedule_cargo_uninstall() -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .arg("/C")
        .arg("ping -n 3 127.0.0.1 >nul & cargo uninstall wb300")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
}

/// Remove machine-wide state/config and wb300's registry keys. Best-effort;
/// returns whether everything that existed was removed.
#[cfg(windows)]
fn purge_windows() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let mut ok = true;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for key in [
        r"Software\WB300",
        r"Software\Classes\AppUserModelId\QubeTX.WB300",
    ] {
        match hkcu.delete_subkey_all(key) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => ok = false,
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let dir = std::path::PathBuf::from(local).join("wb300");
        if dir.exists() && std::fs::remove_dir_all(&dir).is_err() {
            ok = false;
        }
    }
    ok
}

// ---------------------------------------------------------------------------
// Unix
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn perform(purge: bool) -> Outcome {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(err) => {
            return Outcome {
                ok: false,
                origin: "unknown".to_string(),
                action: format!("Could not locate the running binary: {err}"),
                purged: false,
            };
        }
    };
    let path = exe.to_string_lossy().to_string();

    let (ok, origin, action) = if path.contains("/.cargo/bin/") {
        // Unix allows removing a running binary, so cargo can do it directly.
        match std::process::Command::new("cargo")
            .args(["uninstall", "wb300"])
            .status()
        {
            Ok(s) if s.success() => (
                true,
                "cargo-or-installer".to_string(),
                "Removed via `cargo uninstall wb300`.".to_string(),
            ),
            Ok(s) => (
                false,
                "cargo-or-installer".to_string(),
                format!("`cargo uninstall wb300` exited with {s}."),
            ),
            Err(err) => (
                false,
                "cargo-or-installer".to_string(),
                format!("Could not run cargo: {err}. Delete {path} manually."),
            ),
        }
    } else {
        match std::fs::remove_file(&exe) {
            Ok(()) => (true, "binary".to_string(), format!("Removed {path}.")),
            Err(err) => (
                false,
                "binary".to_string(),
                format!("Could not remove {path}: {err}."),
            ),
        }
    };

    let purged = purge && purge_unix();
    Outcome {
        ok,
        origin,
        action,
        purged,
    }
}

/// Remove the machine-wide state and config directories. Best-effort.
#[cfg(not(windows))]
fn purge_unix() -> bool {
    let mut ok = true;
    let mut dirs: Vec<std::path::PathBuf> = vec![crate::home::home_state_dir()];
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        dirs.push(std::path::PathBuf::from(xdg).join("wb300"));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(std::path::PathBuf::from(home).join(".config").join("wb300"));
    }
    for dir in dirs {
        if dir.exists() && std::fs::remove_dir_all(&dir).is_err() {
            ok = false;
        }
    }
    ok
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn msi_repair_strings_are_rewritten_to_uninstall() {
        // The Global MSI's real-world UninstallString uses /I — repair mode.
        assert_eq!(
            msi_uninstall_command("MsiExec.exe /I{25F2F930-45D8-4D82-9B34-AF000887C395}"),
            "MsiExec.exe /X{25F2F930-45D8-4D82-9B34-AF000887C395}"
        );
        assert_eq!(
            msi_uninstall_command("MsiExec.exe /X{AAAA-BBBB}"),
            "MsiExec.exe /X{AAAA-BBBB}"
        );
        assert_eq!(
            msi_uninstall_command("msiexec.exe /x {CCCC-DDDD}"),
            "MsiExec.exe /X{CCCC-DDDD}"
        );
        // No GUID → leave untouched.
        assert_eq!(msi_uninstall_command("whatever.exe"), "whatever.exe");
    }

    #[test]
    fn splits_quoted_and_unquoted_command_lines() {
        let (p, a) = split_command_line(r#""C:\Program Files\wb300\unins000.exe" /SILENT"#);
        assert_eq!(p, r"C:\Program Files\wb300\unins000.exe");
        assert_eq!(a, vec!["/SILENT"]);

        let (p, a) = split_command_line("MsiExec.exe /X{ABCD-1234}");
        assert_eq!(p, "MsiExec.exe");
        assert_eq!(a, vec!["/X{ABCD-1234}"]);

        let (p, a) = split_command_line("solo.exe");
        assert_eq!(p, "solo.exe");
        assert!(a.is_empty());
    }
}
