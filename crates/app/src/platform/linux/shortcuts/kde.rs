//! KDE's public command-shortcut contract: load a service via a temporary action,
//! remove that action, assign _launch and read back shortcutKeys. See linux-status.md.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::platform::linux::shortcut_combo::qt_text;
use ashpd::zbus::{Connection, Proxy};

// QKeySequence is a structure containing an array of four combined Qt keys: (ai).
type Sequences = Vec<(Vec<i32>,)>;
// Public KGlobalShortcutInfo wire fields: action/name, component/name, context/name, keys/defaults.
type ShortcutInfo = (String, String, String, String, String, String, Vec<i32>, Vec<i32>);

fn availability_args(key: i32, match_type: i32) -> ((Vec<i32>,), (i32,)) {
    ((vec![key, 0, 0, 0],), (match_type,))
}
fn foreign_conflict(infos: &[ShortcutInfo], owned: &[&str]) -> bool {
    infos.iter().any(|info| !owned.contains(&info.2.as_str()))
}

fn action_args(id: &str, action: &str) -> Vec<String> {
    vec![id.into(), action.into(), "Opit Speech to Text".into(), action.into()]
}
fn shortcut_args(id: &str, key: i32) -> (Vec<String>, Sequences) {
    (action_args(id, "_launch"), vec![(vec![key, 0, 0, 0],)])
}
fn config_args(id: &str, combo: Option<&str>) -> Vec<String> {
    let mut args =
        ["--file", "kglobalshortcutsrc", "--group", "services", "--group", id, "--key", "_launch", "--notify"]
            .map(str::to_owned)
            .to_vec();
    if let Some(combo) = combo {
        args.push(combo.into());
    } else {
        args.push("--delete".into());
    }
    args
}
fn write_config(id: &str, combo: Option<&str>) -> Result<(), &'static str> {
    let args = config_args(id, combo);
    helper("kwriteconfig6", &args.iter().map(String::as_str).collect::<Vec<_>>()).map(|_| ())
}
fn read_config(id: &str) -> Result<Option<String>, &'static str> {
    let value = helper(
        "kreadconfig6",
        &["--file", "kglobalshortcutsrc", "--group", "services", "--group", id, "--key", "_launch"],
    )?;
    let value = value.trim();
    Ok((!value.is_empty() && value != "none").then(|| value.to_owned()))
}

fn owned_files(dir: &Path, action: &str) -> Result<Vec<PathBuf>, &'static str> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Could not read shortcut desktop entries"),
    };
    let prefix = action_prefix(action);
    let marker = format!("X-Opit-Shortcut-Action={action}");
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| "Could not read shortcut desktop entries")?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(&prefix) || !name.to_string_lossy().ends_with(".desktop") {
            continue;
        }
        if !entry.file_type().map_err(|_| "Could not inspect shortcut desktop entry")?.is_file() {
            return Err("Untrusted shortcut desktop entry");
        }
        let text = std::fs::read_to_string(entry.path()).map_err(|_| "Could not read shortcut desktop entry")?;
        if text.lines().any(|line| line == marker) {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}
fn file_id(file: &Path) -> Result<&str, &'static str> {
    file.file_name().and_then(|s| s.to_str()).ok_or("Invalid shortcut desktop ID")
}
async fn proxy() -> Result<Proxy<'static>, &'static str> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let connection = Connection::session().await.map_err(|_| "KGlobalAccel is unavailable")?;
        Proxy::new_owned(connection, "org.kde.kglobalaccel", "/kglobalaccel", "org.kde.KGlobalAccel")
            .await
            .map_err(|_| "KGlobalAccel is unavailable")
    })
    .await
    .map_err(|_| "KGlobalAccel timed out")?
}
async fn registered(proxy: &Proxy<'_>, id: &str) -> Result<Sequences, &'static str> {
    proxy.call("shortcutKeys", &(action_args(id, "_launch"),)).await.map_err(|_| "KGlobalAccel read failed")
}
async fn available(proxy: &Proxy<'_>, owned: &[&str], key: i32) -> Result<bool, &'static str> {
    // Include multi-chord shadowing as well as exact conflicts; never remove a foreign binding.
    for match_type in 0..=2 {
        let infos: Vec<ShortcutInfo> = proxy
            .call("globalShortcutsByKey", &availability_args(key, match_type))
            .await
            .map_err(|_| "KGlobalAccel conflict check failed")?;
        if foreign_conflict(&infos, owned) {
            return Ok(false);
        }
    }
    Ok(true)
}
async fn install(proxy: &Proxy<'_>, id: &str, key: i32) -> Result<(), &'static str> {
    let dummy = action_args(id, "");
    let _: () = proxy.call("doRegister", &(dummy,)).await.map_err(|_| "KGlobalAccel registration failed")?;
    let _: bool = proxy.call("unregister", &(id, "")).await.map_err(|_| "KGlobalAccel registration failed")?;
    let args = shortcut_args(id, key);
    let _: () = proxy.call("setForeignShortcutKeys", &args).await.map_err(|_| "KGlobalAccel registration failed")?;
    // NoAutoloading | SetPresent (4 | 2). Read back: a successful void call alone proves nothing.
    let active: Sequences = proxy
        .call("setShortcutKeys", &(args.0, args.1, 6_u32))
        .await
        .map_err(|_| "KGlobalAccel registration failed")?;
    if active.iter().any(|(keys,)| keys.first() == Some(&key))
        && registered(proxy, id).await?.iter().any(|(keys,)| keys.first() == Some(&key))
    {
        Ok(())
    } else {
        Err("KGlobalAccel did not activate the requested shortcut")
    }
}
async fn unregister(proxy: &Proxy<'_>, id: &str) -> Result<(), &'static str> {
    let _: bool = proxy.call("unregister", &(id, "_launch")).await.map_err(|_| "KGlobalAccel removal failed")?;
    if registered(proxy, id).await?.iter().any(|(keys,)| keys.iter().any(|key| *key != 0)) {
        return Err("KGlobalAccel removal was not confirmed");
    }
    Ok(())
}

pub(super) async fn manage(
    action: &str,
    exe: &Path,
    data: Option<&Path>,
    combo: Option<&Combo>,
    operation: Operation,
    state: &mut RegistryState,
) -> Result<(), &'static str> {
    let dir = dirs::data_dir().ok_or("Desktop applications directory is unavailable")?.join("applications");
    let old_files = owned_files(&dir, action)?;
    state.owned = !old_files.is_empty();
    if let Some(combo) = combo {
        let expected = dir.join(desktop_id(action, &state.command, &combo.kde()));
        state.command_matches = old_files.len() == 1
            && old_files[0] == expected
            && std::fs::read_to_string(&expected).ok().as_deref() == Some(&desktop_entry(exe, action, data)?);
    }
    if old_files.is_empty() && combo.is_none() && !matches!(operation, Operation::Install) {
        return Ok(());
    }
    let proxy = proxy().await.ok();
    let mut conflict_checked = false;
    if let (Some(proxy), Some(combo)) = (&proxy, combo) {
        let owned = old_files.iter().filter_map(|p| file_id(p).ok()).collect::<Vec<_>>();
        if let Ok(free) = tokio::time::timeout(Duration::from_secs(3), available(proxy, &owned, combo.qt()))
            .await
            .unwrap_or(Err("KGlobalAccel timed out"))
        {
            state.conflict = !free;
            conflict_checked = true;
        }
    }
    if matches!(operation, Operation::Install) && !state.conflict {
        let combo = combo.ok_or("Invalid Linux shortcut")?;
        let text = combo.kde();
        let id = desktop_id(action, &state.command, &text);
        let file = dir.join(&id);
        let content = desktop_entry(exe, action, data)?;
        std::fs::create_dir_all(&dir).map_err(|_| "Could not create shortcut applications directory")?;
        if std::fs::symlink_metadata(&file).is_ok() && !old_files.contains(&file) {
            return Err("Shortcut desktop entry belongs to another application");
        }
        std::fs::write(&file, content).map_err(|_| "Could not write shortcut desktop entry")?;
        // Ensure serviceByStorageId sees the new entry before doRegister. Failure uses login fallback.
        let cache_ready = helper("kbuildsycoca6", &["--noincremental"]).is_ok();
        let live = if cache_ready
            && conflict_checked
            && let Some(proxy) = &proxy
        {
            tokio::time::timeout(Duration::from_secs(5), install(proxy, &id, combo.qt())).await.is_ok_and(|r| r.is_ok())
        } else {
            false
        };
        write_config(&id, Some(&text))?;
        if read_config(&id)?.as_deref() != Some(&text) {
            return Err("KDE shortcut configuration readback failed");
        }
        state.registered = Some(text);
        state.after_login = !live;
        for old in &old_files {
            if old == &file {
                continue;
            }
            if let Some(proxy) = &proxy {
                let removed = tokio::time::timeout(Duration::from_secs(3), unregister(proxy, file_id(old)?)).await;
                state.after_login |= !removed.is_ok_and(|r| r.is_ok());
            } else {
                state.after_login = true;
            }
            write_config(file_id(old)?, None)?;
            std::fs::remove_file(old).map_err(|_| "Could not remove old shortcut desktop entry")?;
        }
        return Ok(());
    }
    if matches!(operation, Operation::Remove) {
        for file in &old_files {
            if let Some(proxy) = &proxy {
                state.after_login |= !tokio::time::timeout(Duration::from_secs(3), unregister(proxy, file_id(file)?))
                    .await
                    .is_ok_and(|r| r.is_ok());
            } else {
                state.after_login = true;
            }
            write_config(file_id(file)?, None)?;
            std::fs::remove_file(file).map_err(|_| "Could not remove shortcut desktop entry")?;
        }
        return Ok(());
    }
    for file in old_files {
        let saved = read_config(file_id(&file)?)?;
        let live = if let Some(proxy) = &proxy {
            tokio::time::timeout(Duration::from_secs(3), registered(proxy, file_id(&file)?))
                .await
                .ok()
                .and_then(Result::ok)
        } else {
            None
        };
        let active = live.as_ref().and_then(|sequences| {
            sequences.iter().find_map(|(keys,)| keys.first().filter(|key| **key != 0).map(|key| qt_text(*key)))
        });
        state.after_login |= saved.is_some() && active != saved;
        state.registered = if state.after_login { saved } else { active.or(saved) };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dbus_arguments_use_the_service_launch_action_and_qt_sequence_structure() {
        let (id, sequences) = shortcut_args("opit.desktop", 0x0700_0038);
        assert_eq!(id, ["opit.desktop", "_launch", "Opit Speech to Text", "_launch"]);
        assert_eq!(sequences, vec![(vec![0x0700_0038, 0, 0, 0],)]);
        assert_eq!(action_args("opit.desktop", "")[1], "");
        use ashpd::zbus::zvariant::Type;
        assert_eq!(<Sequences as Type>::SIGNATURE.to_string(), "a(ai)");
        assert_eq!(<(Vec<String>, Sequences, u32) as Type>::SIGNATURE.to_string(), "(asa(ai)u)");
    }
    #[test]
    fn availability_arguments_and_conflicts_preserve_all_foreign_components() {
        use ashpd::zbus::zvariant::Type;
        assert_eq!(<((Vec<i32>,), (i32,)) as Type>::SIGNATURE.to_string(), "((ai)(i))");
        assert_eq!(<Vec<ShortcutInfo> as Type>::SIGNATURE.to_string(), "a(ssssssaiai)");
        assert_eq!(availability_args(0x0700_0038, 0), ((vec![0x0700_0038, 0, 0, 0],), (0,)));
        let info = |id: &str| {
            ("_launch".into(), "".into(), id.into(), "".into(), "default".into(), "".into(), vec![0x0700_0038], vec![])
        };
        assert!(!foreign_conflict(
            &[info("old-opit.desktop"), info("new-opit.desktop")],
            &["old-opit.desktop", "new-opit.desktop"]
        ));
        assert!(foreign_conflict(&[info("old-opit.desktop"), info("other-app.desktop")], &["old-opit.desktop"]));
    }
    #[test]
    fn config_arguments_scope_every_write_to_one_owned_service() {
        let args = config_args("opit.desktop", Some("Ctrl+Shift+F9"));
        assert_eq!(
            args,
            [
                "--file",
                "kglobalshortcutsrc",
                "--group",
                "services",
                "--group",
                "opit.desktop",
                "--key",
                "_launch",
                "--notify",
                "Ctrl+Shift+F9"
            ]
        );
        assert_eq!(config_args("opit.desktop", None).last().unwrap(), "--delete");
    }
    #[test]
    fn removing_or_changing_can_only_discover_our_exact_action_files() {
        let dir = tempfile::tempdir().unwrap();
        let id = desktop_id("dictation", "command", "Ctrl+F9");
        std::fs::write(dir.path().join(&id), desktop_entry(Path::new("/tmp/app"), "dictation", None).unwrap()).unwrap();
        std::fs::write(dir.path().join("other.desktop"), "other app").unwrap();
        std::fs::write(
            dir.path().join(desktop_id("pause", "command", "Ctrl+F10")),
            desktop_entry(Path::new("/tmp/app"), "pause", None).unwrap(),
        )
        .unwrap();
        assert_eq!(owned_files(dir.path(), "dictation").unwrap(), [dir.path().join(id)]);
    }
}
