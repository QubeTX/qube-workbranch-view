//! Classify a process into a [`ProcessLabel`] from its name and command line.
//!
//! Defaults are built in for now; the config subsystem makes the maps
//! user-overridable (handoff §12.6).

use super::types::ProcessLabel;

const AGENTS: &[&str] = &[
    "claude", "codex", "aider", "cursor", "copilot", "gemini", "goose", "cline", "opencode",
];
const TASKS: &[&str] = &[
    "cargo", "npm", "pnpm", "yarn", "bun", "make", "just", "ninja", "gradle", "mvn", "tsc",
];
const RUNTIMES: &[&str] = &[
    "node", "deno", "python", "python3", "ruby", "java", "dotnet", "rustc", "go",
];
const SHELLS: &[&str] = &[
    "bash",
    "zsh",
    "sh",
    "fish",
    "pwsh",
    "powershell",
    "cmd",
    "nu",
    "elvish",
];
const EDITORS: &[&str] = &[
    "vim", "nvim", "vi", "emacs", "nano", "hx", "helix", "code", "codium",
];

/// Classify a process from its executable name and full command line.
pub fn classify(name: &str, cmd: &str) -> ProcessLabel {
    let stem = exe_stem(name);
    let stem = stem.as_str();

    // Claude Desktop (the Electron GUI app and its gpu/renderer/crashpad
    // helpers) is named `claude(.exe)` but is NOT a coding agent — exclude it
    // so it never shows as an agent or makes a worktree look "active".
    if (AGENTS.contains(&stem) || cmd_mentions_agent(cmd)) && !is_desktop_gui(cmd) {
        return ProcessLabel::Agent;
    }
    if TASKS.contains(&stem) {
        return ProcessLabel::Task;
    }
    if RUNTIMES.contains(&stem) {
        return ProcessLabel::Runtime;
    }
    if SHELLS.contains(&stem) {
        return ProcessLabel::Shell;
    }
    if EDITORS.contains(&stem) {
        return ProcessLabel::Editor;
    }
    ProcessLabel::Unknown
}

/// Lowercased final path component with a trailing `.exe`/`.cmd`/`.bat` removed.
fn exe_stem(token: &str) -> String {
    let lower = token.to_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(lower.as_str());
    base.strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".cmd"))
        .or_else(|| base.strip_suffix(".bat"))
        .unwrap_or(base)
        .to_string()
}

/// True for Claude Desktop (the Electron GUI app), so it is not mistaken for a
/// coding agent. Detected by the Electron child-process marker (`--type=…`, used
/// by the gpu/renderer/utility/crashpad helpers) and the packaged-app path
/// (Windows Store `WindowsApps\Claude_…`, macOS `Claude.app/`). Real agent CLIs
/// (`.local/bin/claude.exe`, `…/Claude/claude-code/<ver>/claude.exe`,
/// `claude-agent-sdk`) carry none of these and stay classified as agents.
fn is_desktop_gui(cmd: &str) -> bool {
    let c = cmd.to_lowercase();
    c.contains("--type=")
        || c.contains("windowsapps\\claude_")
        || c.contains("windowsapps/claude_")
        || c.contains("claude.app/")
}

/// True when a command-line argument's own basename is an agent (e.g.
/// `node .../claude.js`). Deliberately checks each arg's *basename* — not raw
/// path components — so a directory like `/home/claude/...` is not a match.
fn cmd_mentions_agent(cmd: &str) -> bool {
    cmd.split_whitespace().any(|arg| {
        let base = exe_stem(arg);
        let stem = base.strip_suffix(".js").unwrap_or(base.as_str());
        AGENTS.contains(&stem)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_agents_by_name() {
        assert_eq!(classify("claude", "claude"), ProcessLabel::Agent);
        assert_eq!(
            classify("claude.exe", "C:\\Users\\x\\claude.exe chat"),
            ProcessLabel::Agent
        );
        assert_eq!(classify("codex", "codex"), ProcessLabel::Agent);
    }

    #[test]
    fn excludes_claude_desktop_gui() {
        // Desktop main window (Windows Store package) — not a coding agent.
        assert_eq!(
            classify(
                "claude.exe",
                "\"C:\\Program Files\\WindowsApps\\Claude_1.11187.4.0_x64__pzs8sxrjxfjjc\\app\\claude.exe\""
            ),
            ProcessLabel::Unknown
        );
        // Electron helper processes (gpu / crashpad).
        assert_eq!(
            classify(
                "claude.exe",
                "claude.exe --type=gpu-process --user-data-dir=x"
            ),
            ProcessLabel::Unknown
        );
        assert_eq!(
            classify(
                "claude.exe",
                "claude.exe --type=crashpad-handler --database=x"
            ),
            ProcessLabel::Unknown
        );
        // macOS Desktop app bundle.
        assert_eq!(
            classify("Claude", "/Applications/Claude.app/Contents/MacOS/Claude"),
            ProcessLabel::Unknown
        );
    }

    #[test]
    fn keeps_real_claude_code_clis_as_agents() {
        // Installed CLI.
        assert_eq!(
            classify("claude.exe", "\"C:\\Users\\hey\\.local\\bin\\claude.exe\""),
            ProcessLabel::Agent
        );
        // Desktop-spawned headless agent ("local agent mode") — a real agent.
        assert_eq!(
            classify(
                "claude.exe",
                "C:\\Users\\hey\\AppData\\Roaming\\Claude\\claude-code\\2.1.165\\claude.exe --output-format stream-json --model claude-opus-4-8"
            ),
            ProcessLabel::Agent
        );
    }

    #[test]
    fn recognizes_agent_launched_via_runtime() {
        assert_eq!(
            classify("node", "node /home/u/.local/share/claude.js --serve"),
            ProcessLabel::Agent
        );
    }

    #[test]
    fn does_not_match_agent_named_directory() {
        // "claude" is a directory here, not the program — must stay Runtime.
        assert_eq!(
            classify("node", "node /home/claude/app/server.js"),
            ProcessLabel::Runtime
        );
    }

    #[test]
    fn classifies_other_kinds() {
        assert_eq!(classify("cargo", "cargo test"), ProcessLabel::Task);
        assert_eq!(classify("node", "node server.js"), ProcessLabel::Runtime);
        assert_eq!(classify("bash", "bash -l"), ProcessLabel::Shell);
        assert_eq!(classify("nvim", "nvim src/main.rs"), ProcessLabel::Editor);
        assert_eq!(classify("zoxide", "zoxide query"), ProcessLabel::Unknown);
    }
}
