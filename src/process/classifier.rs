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

    if AGENTS.contains(&stem) || cmd_mentions_agent(cmd) {
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
