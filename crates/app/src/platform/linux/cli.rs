//! Headless command relay, dispatched before GTK, single-instance and app-data startup.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(crate) const FLAG: &str = "--cli";
pub(crate) const SHORTCUT_FLAG: &str = "--shortcut";

fn bundled_cli(appdir: &Path) -> Option<PathBuf> {
    appdir.is_absolute().then(|| appdir.join("usr/lib").join(env!("CARGO_PKG_NAME")).join("opit"))
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn launch_command(image: &Path) -> Option<String> {
    image.is_absolute().then(|| format!("{} {FLAG}", shell_quote(&image.to_string_lossy())))
}

pub(crate) fn executable() -> Result<PathBuf, &'static str> {
    let path = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)
        .map_err(|_| "Could not locate the app executable")?;
    if !path.is_absolute() || path.to_str().is_none_or(|s| s.chars().any(char::is_control)) {
        return Err("The app executable needs an absolute UTF-8 path without control characters");
    }
    Ok(path)
}

pub(crate) fn shortcut_command(exe: &Path, action: &str) -> Result<String, &'static str> {
    let exe = exe
        .to_str()
        .filter(|s| exe.is_absolute() && !s.chars().any(char::is_control))
        .ok_or("Invalid executable path")?;
    if !opit_ipc::valid_shortcut_action(action) {
        return Err("Unknown shortcut action");
    }
    Ok(format!("{} {SHORTCUT_FLAG} {}", shell_quote(exe), shell_quote(action)))
}

/// Preserve an explicit scratch instance when the desktop launches with its own environment.
pub(crate) fn data_override() -> Result<Option<PathBuf>, &'static str> {
    std::env::var_os("OPIT_DATA_DIR")
        .map(|dir| {
            let path = PathBuf::from(dir);
            if path.is_absolute() {
                Ok(path)
            } else {
                std::env::current_dir().map(|cwd| cwd.join(path)).map_err(|_| "Could not resolve the data override")
            }
        })
        .transpose()
}

pub(crate) fn data_argument(dir: &Path) -> Result<String, &'static str> {
    let dir = dir
        .to_str()
        .filter(|s| dir.is_absolute() && !s.chars().any(char::is_control))
        .ok_or("Invalid data override path")?;
    Ok(format!("OPIT_DATA_DIR={dir}"))
}

pub(crate) fn shortcut_command_with_data(
    exe: &Path,
    action: &str,
    data: Option<&Path>,
) -> Result<String, &'static str> {
    let command = shortcut_command(exe, action)?;
    match data {
        Some(dir) => Ok(format!("/usr/bin/env {} {command}", shell_quote(&data_argument(dir)?))),
        None => Ok(command),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Gui,
    BundledCli(Vec<OsString>),
    Shortcut(String),
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Entry, &'static str> {
    let mut args = args.into_iter();
    match args.next().as_deref().and_then(|s| s.to_str()) {
        Some(SHORTCUT_FLAG) => {
            let action = args
                .next()
                .and_then(|s| s.into_string().ok())
                .filter(|s| opit_ipc::valid_shortcut_action(s))
                .ok_or("Expected --shortcut <action>")?;
            if args.next().is_some() {
                return Err("Unexpected shortcut arguments");
            }
            Ok(Entry::Shortcut(action))
        }
        Some(FLAG) => Ok(Entry::BundledCli(args.collect())),
        _ => Ok(Entry::Gui),
    }
}

pub(crate) async fn relay(action: String) -> Result<(), opit_ipc::Error> {
    use opit_ipc::{Command, Outcome, Request};
    let path = opit_ipc::unix::socket_path()
        .map_err(|_| opit_ipc::Error::new("app_not_running", "No local app session is available."))?;
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        opit_ipc::unix::call_at(&path, &Request::new(Command::Shortcut { action })),
    )
    .await
    .map_err(|_| opit_ipc::Error::new("timeout", "Shortcut relay timed out."))??;
    match response.outcome {
        Outcome::Ok { .. } => Ok(()),
        Outcome::Err { error } => Err(error),
    }
}

pub(crate) fn dispatch() {
    use std::os::unix::process::CommandExt;
    let entry = match parse(std::env::args_os().skip(1)) {
        Ok(entry) => entry,
        Err(message) => {
            eprintln!("Opit Speech to Text: {message}");
            std::process::exit(2);
        }
    };
    let action = match entry {
        Entry::Gui => return,
        Entry::BundledCli(args) => {
            if let Some(path) = std::env::var_os("APPDIR").and_then(|dir| bundled_cli(Path::new(&dir))) {
                let _ = std::process::Command::new(path).args(args).exec();
            }
            eprintln!("Opit Speech to Text: the bundled CLI is unavailable; run the installed opit command");
            std::process::exit(1);
        }
        Entry::Shortcut(action) => action,
    };
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| opit_ipc::Error::new("unavailable", "Shortcut runtime unavailable."))
        .and_then(|runtime| runtime.block_on(relay(action)));
    // A stale desktop binding must never launch the GUI or change real app data.
    match result {
        Ok(()) => std::process::exit(0),
        Err(error) if error.code == "app_not_running" => std::process::exit(0),
        Err(_) => {
            eprintln!("Opit Speech to Text: shortcut command failed");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn scratch_instance_commands_keep_the_absolute_data_override() {
        let command =
            shortcut_command_with_data(Path::new("/tmp/app"), "dictation", Some(Path::new("/tmp/Opit's $scratch")))
                .unwrap();
        assert_eq!(command, "/usr/bin/env 'OPIT_DATA_DIR=/tmp/Opit'\\''s $scratch' '/tmp/app' --shortcut 'dictation'");
        assert!(shortcut_command_with_data(Path::new("/tmp/app"), "dictation", Some(Path::new("relative"))).is_err());
    }
    #[test]
    fn shortcut_relay_never_falls_through_to_gui_on_bad_arguments() {
        for values in [vec!["--shortcut"], vec!["--shortcut", "unknown"], vec!["--shortcut", "dictation", "extra"]] {
            assert!(parse(args(&values)).is_err());
        }
        for action in ["dictation", "pause", "cancel", "ask", "edit", "translate", "style_cycle", "style:formal"] {
            assert_eq!(parse(args(&["--shortcut", action])).unwrap(), Entry::Shortcut(action.into()));
        }
        assert_eq!(parse(args(&[])).unwrap(), Entry::Gui);
        assert_eq!(parse(args(&["--cli", "toggle"])).unwrap(), Entry::BundledCli(args(&["toggle"])));
    }
    #[test]
    fn persistent_commands_support_every_install_format_and_quote_paths() {
        for exe in [
            "/usr/bin/opit-speech-to-text",
            "/tmp/target/agent-build/release/opit-speech-to-text",
            "/home/test/Opit's App.AppImage",
        ] {
            let command = shortcut_command(Path::new(exe), "style:formal").unwrap();
            assert!(command.ends_with("--shortcut 'style:formal'"));
            assert!(!command.contains("/tmp/.mount_"));
        }
        assert_eq!(
            launch_command(Path::new("/home/test/Opit's App.AppImage")).unwrap(),
            "'/home/test/Opit'\\''s App.AppImage' --cli"
        );
        assert!(shortcut_command(Path::new("relative"), "dictation").is_err());
        assert!(shortcut_command(Path::new("/tmp/app\nExec=bad"), "dictation").is_err());
        assert!(shortcut_command(Path::new("/tmp/app"), "style:../bad").is_err());
        assert_eq!(
            bundled_cli(Path::new("/tmp/.mount_opit")).unwrap(),
            Path::new("/tmp/.mount_opit/usr/lib/opit-speech-to-text/opit")
        );
    }
}
