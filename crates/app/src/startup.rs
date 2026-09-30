//! Data locations and the start-up loading that must never stop the app from opening:
//! a broken `config.json` is backed up and replaced by defaults, a broken `user.yaml`
//! is skipped.

use std::path::{Path, PathBuf};

use opit_core::config::{APP_DIR_NAME, AppConfig};
use opit_core::rules::{RulePack, load_pack_file};
use tracing::{info, warn};

/// Overrides the data folder (development and manual tests).
pub const DATA_DIR_ENV: &str = "OPIT_DATA_DIR";

/// `%APPDATA%\opit-speech-to-text\…`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub user_rules: PathBuf,
    pub history_db: PathBuf,
    pub audio: PathBuf,
    pub logs: PathBuf,
}

impl Paths {
    pub fn under(root: PathBuf) -> Self {
        Self {
            config: root.join("config.json"),
            user_rules: root.join("rules").join("user.yaml"),
            history_db: root.join("history.db"),
            audio: root.join("audio"),
            logs: root.join("logs"),
            root,
        }
    }

    /// `OPIT_DATA_DIR` when set (development, manual tests), else roaming AppData.
    pub fn from_system() -> Option<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            return Some(Self::under(PathBuf::from(dir)));
        }
        dirs::config_dir().map(|dir| Self::under(dir.join(APP_DIR_NAME)))
    }
}

/// Something the user should hear about once the app is up.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartupNotice {
    ConfigReset { backup: Option<PathBuf>, reason: String },
    UserRulesBroken { reason: String },
}

pub struct LoadedConfig {
    pub config: AppConfig,
    /// No config existed; defaults were written (first run).
    pub created: bool,
    pub notice: Option<StartupNotice>,
}

pub fn load_config(path: &Path, now_ms: i64) -> LoadedConfig {
    let existed = path.exists();
    match AppConfig::load(path) {
        Ok(config) if existed => LoadedConfig { config, created: false, notice: None },
        Ok(config) => {
            if let Err(err) = config.save(path) {
                warn!(error = %err, "could not write the default config");
            }
            info!("first run: default config written");
            LoadedConfig { config, created: true, notice: None }
        }
        Err(err) => {
            warn!(error = %err, "config.json is unusable; starting from defaults");
            let backup = move_config_aside(path, now_ms).ok();
            let config = AppConfig::default();
            // Never overwrite a file we could not move aside: it may be fine, just locked.
            if backup.is_some()
                && let Err(err) = config.save(path)
            {
                warn!(error = %err, "could not write the default config");
            }
            let notice = StartupNotice::ConfigReset { backup, reason: err.to_string() };
            LoadedConfig { config, created: false, notice: Some(notice) }
        }
    }
}

/// Moves an unusable config file to `config.json.bad-<now_ms>` next to it; returns the new path.
pub fn move_config_aside(path: &Path, now_ms: i64) -> std::io::Result<PathBuf> {
    let backup = path.with_file_name(format!("config.json.bad-{now_ms}"));
    std::fs::rename(path, &backup).map(|()| backup)
}

/// The personal pack, or `None` when it does not exist or cannot be parsed.
pub fn load_user_pack(path: &Path) -> (Option<RulePack>, Option<StartupNotice>) {
    if !path.exists() {
        return (None, None);
    }
    match load_pack_file(path) {
        Ok(pack) => (Some(pack), None),
        Err(err) => {
            warn!(error = %err, "user.yaml skipped");
            (None, Some(StartupNotice::UserRulesBroken { reason: err.to_string() }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join(APP_DIR_NAME));
        (dir, paths)
    }

    #[test]
    fn layout_matches_the_brief() {
        let paths = Paths::under(PathBuf::from("R"));
        assert_eq!(paths.config, PathBuf::from("R").join("config.json"));
        assert_eq!(paths.user_rules, PathBuf::from("R").join("rules").join("user.yaml"));
        assert_eq!(paths.history_db, PathBuf::from("R").join("history.db"));
        assert_eq!(paths.audio, PathBuf::from("R").join("audio"));
        assert_eq!(paths.logs, PathBuf::from("R").join("logs"));
    }

    #[test]
    fn first_run_writes_the_defaults() {
        let (_dir, paths) = paths();
        let loaded = load_config(&paths.config, 1);
        assert!(loaded.created && loaded.notice.is_none());
        assert_eq!(AppConfig::load(&paths.config).unwrap(), AppConfig::default());
    }

    #[test]
    fn a_valid_config_is_loaded_as_is() {
        let (_dir, paths) = paths();
        let mut config = AppConfig::default();
        config.paste.trailing_space = false;
        config.save(&paths.config).unwrap();
        let loaded = load_config(&paths.config, 1);
        assert!(!loaded.created && loaded.notice.is_none());
        assert_eq!(loaded.config, config);
    }

    #[test]
    fn a_broken_config_is_backed_up_and_replaced() {
        let (_dir, paths) = paths();
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(&paths.config, r#"{"profiles":[{"id":"x"}]}"#).unwrap();
        let loaded = load_config(&paths.config, 42);

        assert_eq!(loaded.config, AppConfig::default());
        let backup = paths.root.join("config.json.bad-42");
        match loaded.notice {
            Some(StartupNotice::ConfigReset { backup: Some(path), reason }) => {
                assert_eq!(path, backup);
                assert!(!reason.is_empty());
            }
            other => panic!("unexpected notice: {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), r#"{"profiles":[{"id":"x"}]}"#);
        assert_eq!(AppConfig::load(&paths.config).unwrap(), AppConfig::default());
    }

    #[test]
    fn a_config_from_a_newer_version_is_backed_up_too() {
        let (_dir, paths) = paths();
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(&paths.config, r#"{"schema_version":99}"#).unwrap();
        let loaded = load_config(&paths.config, 7);
        assert!(matches!(loaded.notice, Some(StartupNotice::ConfigReset { backup: Some(_), .. })));
        assert!(paths.root.join("config.json.bad-7").exists());
    }

    #[test]
    fn user_pack_missing_valid_and_broken() {
        let (_dir, paths) = paths();
        assert_eq!(load_user_pack(&paths.user_rules), (None, None));

        std::fs::create_dir_all(paths.user_rules.parent().unwrap()).unwrap();
        std::fs::write(&paths.user_rules, "schema: 1\nid: user\nname: Me\nterms: [Opit]\n").unwrap();
        assert_eq!(load_user_pack(&paths.user_rules).0.unwrap().terms, ["Opit"]);

        std::fs::write(&paths.user_rules, "schema: 1\nid: user\nnme: typo\n").unwrap();
        let (pack, notice) = load_user_pack(&paths.user_rules);
        assert!(pack.is_none());
        assert!(matches!(notice, Some(StartupNotice::UserRulesBroken { .. })));
    }
}
