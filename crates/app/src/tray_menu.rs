//! Tray menu contents as plain data; `tray.rs` turns this into a Tauri menu.

use opit_core::config::AppConfig;

use crate::i18n::Lang;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayItem {
    Action { id: String, label: String, enabled: bool },
    Check { id: String, label: String, checked: bool },
    Submenu { label: String, items: Vec<TrayItem> },
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    Profile(String),
    TogglePause,
    Retry,
    Quit,
}

const PROFILE_PREFIX: &str = "profile:";

pub fn build(config: &AppConfig, lang: Lang, paused: bool, can_retry: bool) -> Vec<TrayItem> {
    let action =
        |id: &str, label: &str, enabled: bool| TrayItem::Action { id: id.into(), label: label.into(), enabled };
    let profiles = config
        .profiles
        .iter()
        .map(|p| TrayItem::Check {
            id: format!("{PROFILE_PREFIX}{}", p.id),
            label: p.name.clone(),
            checked: p.id == config.active_profile_id,
        })
        .collect();
    vec![
        action("open", lang.tray_open(), true),
        TrayItem::Submenu { label: lang.tray_profile().into(), items: profiles },
        TrayItem::Check { id: "pause".into(), label: lang.tray_pause_hotkey().into(), checked: paused },
        action("retry", lang.retry(), can_retry),
        TrayItem::Separator,
        action("quit", lang.tray_quit(), true),
    ]
}

pub fn parse(id: &str) -> Option<TrayCommand> {
    match id {
        "open" => Some(TrayCommand::Open),
        "pause" => Some(TrayCommand::TogglePause),
        "retry" => Some(TrayCommand::Retry),
        "quit" => Some(TrayCommand::Quit),
        _ => id.strip_prefix(PROFILE_PREFIX).map(|p| TrayCommand::Profile(p.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_matches_the_brief() {
        let items = build(&AppConfig::default(), Lang::Tr, true, false);
        let TrayItem::Submenu { label, items: profiles } = &items[1] else { panic!("{items:?}") };
        assert_eq!(label, "Profil");
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0], TrayItem::Check { id: "profile:groq".into(), label: "Groq".into(), checked: true });
        assert_eq!(items[0], TrayItem::Action { id: "open".into(), label: "Aç".into(), enabled: true });
        assert_eq!(items[2], TrayItem::Check { id: "pause".into(), label: "Kısayolu duraklat".into(), checked: true });
        assert_eq!(items[3], TrayItem::Action { id: "retry".into(), label: "Tekrar dene".into(), enabled: false });
        assert_eq!(items[5], TrayItem::Action { id: "quit".into(), label: "Çıkış".into(), enabled: true });
    }

    #[test]
    fn every_id_parses_back() {
        for item in build(&AppConfig::default(), Lang::En, false, true) {
            match item {
                TrayItem::Action { id, .. } | TrayItem::Check { id, .. } => assert!(parse(&id).is_some(), "{id}"),
                TrayItem::Submenu { items, .. } => {
                    for sub in items {
                        let TrayItem::Check { id, .. } = sub else { panic!() };
                        assert_eq!(parse(&id), Some(TrayCommand::Profile(id["profile:".len()..].to_string())));
                    }
                }
                TrayItem::Separator => {}
            }
        }
        assert_eq!(parse("bogus"), None);
    }
}
