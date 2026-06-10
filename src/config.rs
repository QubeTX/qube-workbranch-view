//! Minimal TOML configuration. Today it only carries notification toggles;
//! it is the seed of the fuller config subsystem (handoff §18).
//!
//! Location: `%LOCALAPPDATA%\wb300\config.toml` on Windows,
//! `$XDG_CONFIG_HOME/wb300/config.toml` (or `~/.config/wb300/config.toml`)
//! elsewhere. A missing file means defaults; a malformed file logs one
//! warning and falls back to defaults — config can never prevent startup.
//! Unknown keys are ignored (forward compatibility).

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub notifications: NotifyConfig,
}

/// `[notifications]` — OS toast behavior.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NotifyConfig {
    /// Master switch (also forced off by `--no-notify`).
    pub enabled: bool,
    /// Toast when a branch gets new commits.
    pub commit: bool,
    /// Toast when a branch's work reaches its remote.
    pub push: bool,
    /// Toast when two branches start changing the same file.
    pub conflict_risk: bool,
    /// Per-(repo, branch, kind) suppression window, in seconds.
    pub cooldown_secs: u64,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            commit: true,
            push: true,
            conflict_risk: true,
            cooldown_secs: 30,
        }
    }
}

/// The per-user config file path, if a base directory is resolvable.
pub fn config_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
    } else if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config")
    };
    Some(base.join("wb300").join("config.toml"))
}

/// Load the config: missing file → defaults; malformed file → one warning +
/// defaults. Loaded once at startup (no live reload).
pub fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default(); // missing file is the normal case
    };
    match toml::from_str(&text) {
        Ok(config) => config,
        Err(err) => {
            tracing::warn!("ignoring malformed {}: {err}", path.display());
            Config::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_on_with_30s_cooldown() {
        let c = NotifyConfig::default();
        assert!(c.enabled && c.commit && c.push && c.conflict_risk);
        assert_eq!(c.cooldown_secs, 30);
    }

    #[test]
    fn partial_toml_keeps_defaults_for_the_rest() {
        let c: Config = toml::from_str("[notifications]\npush = false\n").unwrap();
        assert!(c.notifications.enabled);
        assert!(c.notifications.commit);
        assert!(!c.notifications.push);
        assert_eq!(c.notifications.cooldown_secs, 30);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let c: Config =
            toml::from_str("[notifications]\nfuture_option = true\ncommit = false\n").unwrap();
        assert!(!c.notifications.commit);
    }

    #[test]
    fn empty_input_is_default() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.notifications, NotifyConfig::default());
    }
}
