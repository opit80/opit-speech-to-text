//! Tray menu contents as plain data; `tray.rs` turns this into a Tauri menu.

use opit_core::config::AppConfig;

use crate::controller::{DictationState, DictationStatus};
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
    Ask,
    NewConversation,
    Open,
    Profile(String),
    Style(String),
    TogglePause,
    PauseRecording,
    Retry,
    Quit,
}

const PROFILE_PREFIX: &str = "profile:";

pub fn build(config: &AppConfig, lang: Lang, paused: bool, status: &DictationStatus) -> Vec<TrayItem> {
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
        action("retry", lang.retry(), status.can_retry),
        action("pause-recording", lang.tray_recording_pause(status.paused), status.state == DictationState::Recording),
        action("ask", lang.tray_ask(), true),
        action("new-conversation", lang.new_conversation(), true),
        TrayItem::Submenu {
            label: lang.tray_style().into(),
            items: config
                .cleanup
                .styles
                .iter()
                .map(|style| TrayItem::Check {
                    id: format!("style:{}", style.id),
                    label: lang.style_name(style),
                    checked: style.id == config.cleanup.active_style_id,
                })
                .collect(),
        },
        TrayItem::Separator,
        action("quit", lang.tray_quit(), true),
    ]
}

pub fn parse(id: &str) -> Option<TrayCommand> {
    match id {
        "ask" => Some(TrayCommand::Ask),
        "new-conversation" => Some(TrayCommand::NewConversation),
        "open" => Some(TrayCommand::Open),
        "pause" => Some(TrayCommand::TogglePause),
        "pause-recording" => Some(TrayCommand::PauseRecording),
        "retry" => Some(TrayCommand::Retry),
        "quit" => Some(TrayCommand::Quit),
        _ => id
            .strip_prefix(PROFILE_PREFIX)
            .map(|p| TrayCommand::Profile(p.to_string()))
            .or_else(|| id.strip_prefix("style:").map(|s| TrayCommand::Style(s.into()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_matches_the_brief() {
        let items = build(&AppConfig::default(), Lang::Tr, true, &DictationStatus::default());
        let TrayItem::Submenu { label, items: profiles } = &items[1] else { panic!("{items:?}") };
        assert_eq!(label, "Profil");
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0], TrayItem::Check { id: "profile:groq".into(), label: "Groq".into(), checked: true });
        assert_eq!(items[0], TrayItem::Action { id: "open".into(), label: "Aç".into(), enabled: true });
        assert_eq!(items[2], TrayItem::Check { id: "pause".into(), label: "Kısayolu duraklat".into(), checked: true });
        assert_eq!(items[3], TrayItem::Action { id: "retry".into(), label: "Tekrar dene".into(), enabled: false });
        assert_eq!(items[9], TrayItem::Action { id: "quit".into(), label: "Çıkış".into(), enabled: true });
    }

    #[test]
    fn every_id_parses_back() {
        for item in build(&AppConfig::default(), Lang::En, false, &DictationStatus::default()) {
            match item {
                TrayItem::Action { id, .. } | TrayItem::Check { id, .. } => assert!(parse(&id).is_some(), "{id}"),
                TrayItem::Submenu { items, .. } => {
                    for sub in items {
                        let TrayItem::Check { id, .. } = sub else { panic!() };
                        let expected = if let Some(id) = id.strip_prefix("profile:") {
                            TrayCommand::Profile(id.into())
                        } else {
                            TrayCommand::Style(id["style:".len()..].into())
                        };
                        assert_eq!(parse(&id), Some(expected));
                    }
                }
                TrayItem::Separator => {}
            }
        }
        assert_eq!(parse("bogus"), None);
    }

    #[test]
    fn style_submenu_checks_active_style_localizes_defaults_and_preserves_renames() {
        let mut config = AppConfig::default();
        config.cleanup.active_style_id = "formal".into();
        config.cleanup.styles.iter_mut().find(|s| s.id == "casual").unwrap().name = "Personal style".into();
        let items = build(&config, Lang::Tr, false, &DictationStatus::default());
        let TrayItem::Submenu { label, items } = &items[7] else { panic!() };
        assert_eq!(label, "Stil");
        assert_eq!(items.len(), 5);
        assert_eq!(items[0], TrayItem::Check { id: "style:raw".into(), label: "Ham".into(), checked: false });
        assert_eq!(items[2], TrayItem::Check { id: "style:formal".into(), label: "Resmî".into(), checked: true });
        assert_eq!(
            items[3],
            TrayItem::Check { id: "style:casual".into(), label: "Personal style".into(), checked: false }
        );
    }

    #[test]
    fn recording_pause_item_tracks_capture_and_pause_state() {
        for (state, paused, label, enabled) in [
            (DictationState::Idle, false, "Pause recording", false),
            (DictationState::Transcribing, false, "Pause recording", false),
            (DictationState::Recording, false, "Pause recording", true),
            (DictationState::Recording, true, "Resume recording", true),
        ] {
            let status = DictationStatus { state, paused, ..DictationStatus::default() };
            let items = build(&AppConfig::default(), Lang::En, false, &status);
            assert_eq!(items[4], TrayItem::Action { id: "pause-recording".into(), label: label.into(), enabled });
        }
    }
}
