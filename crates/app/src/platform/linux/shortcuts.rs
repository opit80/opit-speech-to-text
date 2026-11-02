//! Desktop-owned command shortcuts. Startup and saved mapping changes reconcile our
//! KDE/GNOME entries on a blocking worker; matching entries require no registry writes.
//! No key capture, portal bindings or privileged input devices.

mod gnome;
mod kde;

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{cli, session, shortcut_combo::Combo};
use crate::platform::{Hotkey, HotkeyError, HotkeyEvent, HotkeySink};

const APP_ID: &str = "io.github.opit80.opit-speech-to-text";
static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Backend {
    Kde,
    Gnome,
    X11,
    Manual,
}

pub(crate) fn classify(desktop: &str, session: session::SessionKind) -> Backend {
    let names = desktop.split(':').collect::<Vec<_>>();
    if names.iter().any(|s| s.eq_ignore_ascii_case("KDE")) {
        Backend::Kde
    } else if names.iter().any(|s| s.eq_ignore_ascii_case("GNOME")) {
        Backend::Gnome
    } else if session == session::SessionKind::X11 {
        Backend::X11
    } else {
        Backend::Manual
    }
}

pub(crate) fn backend() -> Backend {
    classify(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(), session::current())
}

pub(crate) fn event(action: &str) -> Option<HotkeyEvent> {
    Some(match action {
        "dictation" => HotkeyEvent::ComboDown,
        "pause" => HotkeyEvent::Pause,
        "cancel" => HotkeyEvent::Escape,
        "ask" => HotkeyEvent::Ask,
        "edit" => HotkeyEvent::Edit,
        "translate" => HotkeyEvent::Translate,
        "style_cycle" => HotkeyEvent::StyleCycle,
        _ if opit_ipc::valid_shortcut_action(action) => HotkeyEvent::Style(action.strip_prefix("style:")?.into()),
        _ => return None,
    })
}

#[derive(Default)]
pub(crate) struct CommandHotkey {
    primary: Mutex<Vec<String>>,
}

fn action_for(event: &HotkeyEvent) -> Option<String> {
    Some(match event {
        HotkeyEvent::ComboDown => "dictation".into(),
        HotkeyEvent::Pause => "pause".into(),
        HotkeyEvent::Escape => "cancel".into(),
        HotkeyEvent::Ask => "ask".into(),
        HotkeyEvent::Edit => "edit".into(),
        HotkeyEvent::Translate => "translate".into(),
        HotkeyEvent::StyleCycle => "style_cycle".into(),
        HotkeyEvent::Style(id) => format!("style:{id}"),
        _ => return None,
    })
}
/// Installation must use the saved shortcut, including when a form save was rejected.
pub(crate) fn keys_for<'a>(config: &'a opit_core::config::AppConfig, action: &str) -> Option<&'a [String]> {
    Some(match action {
        "dictation" => &config.hotkey.keys,
        "pause" => &config.hotkey.pause_keys,
        "cancel" => &config.hotkey.cancel_keys,
        "ask" => &config.hotkey.ask_keys,
        "edit" => &config.hotkey.edit_keys,
        "translate" => &config.hotkey.translate_keys,
        "style_cycle" => &config.hotkey.style_cycle_keys,
        _ => {
            &config.cleanup.styles.iter().find(|style| Some(style.id.as_str()) == action.strip_prefix("style:"))?.hotkey
        }
    })
}

impl Hotkey for CommandHotkey {
    fn supports_release(&self) -> bool {
        false
    }
    fn register(&self, keys: &[String], _sink: HotkeySink) -> Result<(), HotkeyError> {
        *self.primary.lock().map_err(|_| HotkeyError::Install("Shortcut worker stopped".into()))? = keys.to_vec();
        Ok(())
    }
    fn register_shortcuts(&self, shortcuts: &[(HotkeyEvent, Vec<String>)]) -> Result<(), HotkeyError> {
        let primary = self.primary.lock().map_err(|_| HotkeyError::Install("Shortcut worker stopped".into()))?.clone();
        let actions = std::iter::once(("dictation".to_owned(), primary))
            .chain(shortcuts.iter().filter_map(|(event, keys)| Some((action_for(event)?, keys.clone()))))
            .collect::<Vec<_>>();
        // apply_startup_settings and config saves already run on blocking workers under
        // config_lock. Keep reconciliation ordered with those saves and explicit UI effects.
        reconcile(&actions).map_err(|error| HotkeyError::Install(error.into()))
    }
    fn set_paused(&self, _paused: bool) {}
    fn set_capture_escape(&self, _on: bool) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Operation {
    Read,
    Install,
    Remove,
}

#[derive(Serialize)]
pub(crate) struct RegistryState {
    pub backend: Backend,
    pub registered: Option<String>,
    pub command: String,
    pub after_login: bool,
    pub conflict: bool,
    #[serde(skip)]
    owned: bool,
    #[serde(skip)]
    command_matches: bool,
}

/// Decide from readback, never from the saved config alone. Invalid saved Windows
/// chords remain in config, but must surface through the hotkey status/banner path.
fn reconcile_plan(keys: &[String], state: &RegistryState) -> Result<Option<Operation>, &'static str> {
    if keys.is_empty() {
        return Ok(state.owned.then_some(Operation::Remove));
    }
    let combo = Combo::new(keys).map_err(|_| "Invalid Linux shortcut")?;
    if state.conflict {
        return Err("Desktop shortcut conflicts with another application");
    }
    let expected = match state.backend {
        Backend::Kde => combo.kde(),
        Backend::Gnome => combo.gnome(),
        Backend::X11 | Backend::Manual => return Ok(None),
    };
    Ok((state.registered.as_deref() != Some(&expected) || !state.command_matches || state.after_login)
        .then_some(Operation::Install))
}

fn reconcile(actions: &[(String, Vec<String>)]) -> Result<(), &'static str> {
    if !matches!(backend(), Backend::Kde | Backend::Gnome) {
        return super::shortcut_combo::validate_list(
            &actions.iter().map(|(_, keys)| keys.as_slice()).collect::<Vec<_>>(),
        )
        .map_err(|_| "Invalid Linux shortcut");
    }
    let runtime = tokio::runtime::Handle::try_current().map_err(|_| "Shortcut runtime is unavailable")?;
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "Shortcut registry worker stopped")?;
    runtime.block_on(async {
        let mut error = None;
        let mut seen = Vec::new();
        // Retire cleared actions first so they cannot block a new binding on another action.
        for (action, keys) in actions
            .iter()
            .filter(|(_, keys)| keys.is_empty())
            .chain(actions.iter().filter(|(_, keys)| !keys.is_empty()))
        {
            let result = async {
                if !keys.is_empty() {
                    let combo = Combo::new(keys).map_err(|_| "Invalid Linux shortcut")?.qt();
                    if seen.contains(&combo) {
                        return Err("Desktop shortcut conflicts with another configured action");
                    }
                    seen.push(combo);
                }
                let state = manage_inner(action, keys, Operation::Read).await?;
                if let Some(operation) = reconcile_plan(keys, &state)? {
                    let state = manage_inner(action, keys, operation).await?;
                    if state.conflict {
                        return Err("Desktop shortcut conflicts with another application");
                    }
                    if state.after_login {
                        return Err("Desktop shortcut change requires the next login");
                    }
                }
                Ok(())
            }
            .await;
            // One invalid/unavailable action must not prevent the others from reconciling.
            if let Err(reason) = result {
                error.get_or_insert(reason);
            }
        }
        error.map_or(Ok(()), Err)
    })
}

fn fingerprint(value: &str) -> String {
    // Non-security identifier, stable across builds. Paths never contain user-supplied IDs.
    let hash =
        value.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}

fn action_prefix(action: &str) -> String {
    format!("{APP_ID}.shortcut-{}-", fingerprint(action))
}

fn desktop_id(action: &str, command: &str, combo: &str) -> String {
    // A changed command gets a new service ID, avoiding KService's cached Exec for the old file.
    format!("{}{}.desktop", action_prefix(action), fingerprint(&format!("{command}\n{combo}")))
}

pub(super) fn desktop_exec_arg(value: &str) -> Result<String, &'static str> {
    if value.chars().any(char::is_control) {
        return Err("Invalid desktop command");
    }
    let mut escaped = String::from("\"");
    for c in value.chars() {
        match c {
            // Desktop Entry string unescaping precedes Exec unquoting.
            '\\' => escaped.push_str("\\\\\\\\"),
            '"' | '$' | '`' => {
                escaped.push_str("\\\\");
                escaped.push(c);
            }
            '%' => escaped.push_str("%%"),
            _ => escaped.push(c),
        }
    }
    escaped.push('"');
    Ok(escaped)
}

pub(super) fn desktop_entry(exe: &Path, action: &str, data: Option<&Path>) -> Result<String, &'static str> {
    if !exe.is_absolute() || !opit_ipc::valid_shortcut_action(action) {
        return Err("Invalid desktop command");
    }
    let exe = desktop_exec_arg(exe.to_str().ok_or("Invalid executable path")?)?;
    let action_arg = desktop_exec_arg(action)?;
    let prefix = match data {
        Some(dir) => format!("/usr/bin/env {} ", desktop_exec_arg(&cli::data_argument(dir)?)?),
        None => String::new(),
    };
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Opit Speech to Text ({action})\nNoDisplay=true\nStartupNotify=false\nTerminal=false\nExec={prefix}{exe} --shortcut {action_arg}\nX-KDE-GlobalAccel-CommandShortcut=true\nX-Opit-Shortcut-Action={action}\n"
    ))
}

fn helper(program: &str, args: &[&str]) -> Result<String, &'static str> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "Shortcut registry helper is unavailable")?;
    if !output.status.success() {
        return Err("Shortcut registry helper failed");
    }
    String::from_utf8(output.stdout).map_err(|_| "Shortcut registry returned invalid text")
}

/// Called only from a blocking worker: file/process work and D-Bus never enter GTK's thread.
pub(crate) async fn manage(
    action: String,
    keys: Vec<String>,
    operation: Operation,
) -> Result<RegistryState, &'static str> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let _guard = REGISTRY_LOCK.lock().map_err(|_| "Shortcut registry worker stopped")?;
        runtime.block_on(manage_inner(&action, &keys, operation))
    })
    .await
    .map_err(|_| "Shortcut registry worker stopped")?
}

async fn manage_inner(action: &str, keys: &[String], operation: Operation) -> Result<RegistryState, &'static str> {
    if !opit_ipc::valid_shortcut_action(action) {
        return Err("Unknown shortcut action");
    }
    let exe = cli::executable()?;
    let data = cli::data_override()?;
    let command = cli::shortcut_command_with_data(&exe, action, data.as_deref())?;
    let backend = backend();
    let mut state = RegistryState {
        backend,
        registered: None,
        command,
        after_login: false,
        conflict: false,
        owned: false,
        command_matches: false,
    };
    let combo = Combo::new(keys).ok();
    if matches!(operation, Operation::Install) && combo.is_none() {
        return Err("Invalid Linux shortcut");
    }
    match backend {
        Backend::Kde => kde::manage(action, &exe, data.as_deref(), combo.as_ref(), operation, &mut state).await?,
        Backend::Gnome => gnome::manage(action, combo.as_ref(), operation, &mut state)?,
        Backend::X11 | Backend::Manual => {}
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(backend: Backend) -> RegistryState {
        RegistryState {
            backend,
            registered: None,
            command: "command".into(),
            after_login: false,
            conflict: false,
            owned: false,
            command_matches: false,
        }
    }
    fn keys() -> Vec<String> {
        vec!["LeftCtrl".into(), "F9".into()]
    }
    #[test]
    fn reconcile_installs_missing_or_changed_combos_and_stale_commands_on_both_desktops() {
        for backend in [Backend::Kde, Backend::Gnome] {
            let mut state = state(backend);
            assert_eq!(reconcile_plan(&keys(), &state), Ok(Some(Operation::Install)));
            state.owned = true;
            state.registered = Some("old combo".into());
            state.command_matches = true;
            assert_eq!(reconcile_plan(&keys(), &state), Ok(Some(Operation::Install)));
            state.registered = Some(if backend == Backend::Kde { "Ctrl+F9" } else { "<Control>F9" }.into());
            assert_eq!(reconcile_plan(&keys(), &state), Ok(None));
            state.command_matches = false;
            assert_eq!(reconcile_plan(&keys(), &state), Ok(Some(Operation::Install)));
            state.command_matches = true;
            state.after_login = true;
            assert_eq!(reconcile_plan(&keys(), &state), Ok(Some(Operation::Install)));
        }
    }
    #[test]
    fn reconcile_empty_actions_only_remove_owned_entries_and_repeated_removal_is_a_noop() {
        for backend in [Backend::Kde, Backend::Gnome] {
            let mut state = state(backend);
            assert_eq!(reconcile_plan(&[], &state), Ok(None));
            state.owned = true; // Includes an owned entry whose binding was disabled externally.
            assert_eq!(reconcile_plan(&[], &state), Ok(Some(Operation::Remove)));
            state.owned = false;
            assert_eq!(reconcile_plan(&[], &state), Ok(None));
        }
    }
    #[test]
    fn reconcile_reports_invalid_saved_linux_combos_and_foreign_conflicts() {
        for backend in [Backend::Kde, Backend::Gnome] {
            let mut state = state(backend);
            for invalid in [vec!["A".into()], vec!["RightCtrl".into(), "RightShift".into()]] {
                assert_eq!(reconcile_plan(&invalid, &state), Err("Invalid Linux shortcut"));
            }
            state.conflict = true;
            assert_eq!(reconcile_plan(&keys(), &state), Err("Desktop shortcut conflicts with another application"));
        }
    }
    #[test]
    fn reconciliation_action_names_roundtrip_including_empty_and_style_slots() {
        let config = opit_core::config::AppConfig::default();
        for action in ["dictation", "pause", "cancel", "ask", "edit", "translate", "style_cycle", "style:formal"] {
            assert_eq!(action_for(&event(action).unwrap()).as_deref(), Some(action));
            if !action.starts_with("style:") {
                assert!(keys_for(&config, action).is_some());
            }
        }
        assert_eq!(action_for(&HotkeyEvent::ComboUp), None);
    }
    #[test]
    fn desktop_scratch_override_is_one_quoted_environment_argument() {
        let entry = desktop_entry(Path::new("/tmp/app"), "dictation", Some(Path::new("/tmp/Opit's scratch"))).unwrap();
        assert!(
            entry.contains(
                "Exec=/usr/bin/env \"OPIT_DATA_DIR=/tmp/Opit's scratch\" \"/tmp/app\" --shortcut \"dictation\""
            )
        );
    }
    #[test]
    fn every_configurable_action_has_a_saved_slot_and_press_command() {
        let config = opit_core::config::AppConfig::default();
        for action in ["dictation", "pause", "cancel", "ask", "edit", "translate", "style_cycle"] {
            assert!(keys_for(&config, action).is_some());
            assert!(event(action).is_some());
        }
        for style in &config.cleanup.styles {
            let action = format!("style:{}", style.id);
            assert_eq!(keys_for(&config, &action), Some(style.hotkey.as_slice()));
            assert_eq!(event(&action), Some(HotkeyEvent::Style(style.id.clone())));
        }
        assert_eq!(keys_for(&config, "probe"), None);
        assert_eq!(keys_for(&config, "style:missing"), None);
        assert!(keys_for(&config, "cancel").unwrap().is_empty());
    }
    #[test]
    fn desktop_selection_prioritizes_registries_over_x11() {
        assert_eq!(classify("KDE", session::SessionKind::X11), Backend::Kde);
        assert_eq!(classify("ubuntu:GNOME", session::SessionKind::Wayland), Backend::Gnome);
        assert_eq!(classify("XFCE", session::SessionKind::X11), Backend::X11);
        assert_eq!(classify("sway", session::SessionKind::Wayland), Backend::Manual);
    }
    #[test]
    fn desktop_entry_is_hidden_and_escapes_exec_without_a_shell() {
        let entry = desktop_entry(Path::new("/tmp/Opit's $app` 50% \\\".AppImage"), "style:formal", None).unwrap();
        assert!(entry.contains("NoDisplay=true\n"));
        assert!(entry.contains("X-KDE-GlobalAccel-CommandShortcut=true\n"));
        assert!(entry.contains("--shortcut \"style:formal\""));
        assert!(entry.contains("50%%"));
        assert!(entry.contains("\\\\$app\\\\`"));
        assert!(entry.contains("\\\\\\\\\\\\\""));
        assert!(desktop_entry(Path::new("/tmp/app\nExec=bad"), "dictation", None).is_err());
        assert!(desktop_entry(Path::new("app"), "dictation", None).is_err());
    }
    #[test]
    fn desktop_ids_are_action_scoped_and_change_with_command_or_combo() {
        let a = desktop_id("dictation", "/tmp/app --shortcut dictation", "Ctrl+F9");
        assert_ne!(a, desktop_id("pause", "/tmp/app --shortcut pause", "Ctrl+F9"));
        assert_ne!(a, desktop_id("dictation", "/tmp/new --shortcut dictation", "Ctrl+F9"));
        assert_ne!(a, desktop_id("dictation", "/tmp/app --shortcut dictation", "Ctrl+F10"));
        assert!(a.starts_with(&action_prefix("dictation")));
    }
}
