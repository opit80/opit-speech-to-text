//! In-app updates. `tauri-plugin-updater` reads the GitHub Releases manifest (`plugins > updater`
//! in tauri.conf.json), verifies the minisign signature and runs the new NSIS installer in passive
//! mode. [`UpdateMachine`] holds what the UI shows and is tested on its own; the functions below
//! wire it to the plugin and the `update-state` event.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tracing::{info, warn};

use crate::app_core::CommandError;
use crate::controller::ControllerHandle;
use crate::events;
use crate::settings::SettingsHandle;

/// The first automatic check waits until start-up has settled.
pub const AUTO_CHECK_DELAY: Duration = Duration::from_secs(20);
/// Then the app checks once a day while it keeps running in the tray.
pub const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// The plugin's requests never time out by default, so a stalled one (sleep, captive portal)
/// would keep `Checking` or `Installing` until a restart. The check fetches a small JSON file.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// The whole installer download (under 15 MB); when it runs out, the update can be tried again.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

impl From<&Update> for UpdateInfo {
    fn from(update: &Update) -> Self {
        Self {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone(),
        }
    }
}

/// What the UI shows; payload of the `update-state` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateState {
    /// No check in this run yet.
    Idle,
    Checking,
    UpToDate,
    Available {
        info: UpdateInfo,
    },
    /// Downloading. The app exits when the installer starts.
    Installing {
        info: UpdateInfo,
        downloaded: u64,
        total: Option<u64>,
    },
    /// Offline, no release published yet, a bad manifest … Shown only on Settings → Updates.
    CheckFailed {
        message: String,
    },
}

/// The update state plus the pending update `T` (the plugin's `Update`; a plain value in tests).
pub struct UpdateMachine<T> {
    state: UpdateState,
    pending: Option<T>,
    /// Last progress mark sent to the UI: whole percent, or MiB while the size is unknown.
    reported: Option<u64>,
}

impl<T> Default for UpdateMachine<T> {
    fn default() -> Self {
        Self { state: UpdateState::Idle, pending: None, reported: None }
    }
}

impl<T: Clone> UpdateMachine<T> {
    pub fn state(&self) -> UpdateState {
        self.state.clone()
    }

    /// True when the caller should check now; false while a check or an install is running.
    pub fn begin_check(&mut self) -> bool {
        if matches!(self.state, UpdateState::Checking | UpdateState::Installing { .. }) {
            return false;
        }
        self.state = UpdateState::Checking;
        true
    }

    pub fn finish_check(&mut self, result: Result<Option<(UpdateInfo, T)>, String>) {
        let (state, pending) = match result {
            Ok(Some((info, update))) => (UpdateState::Available { info }, Some(update)),
            Ok(None) => (UpdateState::UpToDate, None),
            Err(message) => (UpdateState::CheckFailed { message }, None),
        };
        self.state = state;
        self.pending = pending;
    }

    /// Hands out the pending update for installing. Refused without one, and while a dictation is
    /// in flight (the installer closes the app).
    pub fn begin_install(&mut self, dictation_busy: bool) -> Result<T, CommandError> {
        let (UpdateState::Available { info }, Some(update)) = (&self.state, &self.pending) else {
            return Err(CommandError::new("unavailable", "there is no update to install; check for updates first"));
        };
        if dictation_busy {
            return Err(CommandError::new("unavailable", "finish the current dictation first"));
        }
        let (info, update) = (info.clone(), update.clone());
        self.state = UpdateState::Installing { info, downloaded: 0, total: None };
        self.reported = None;
        Ok(update)
    }

    /// Adds a downloaded chunk. True when the UI should hear about it.
    pub fn progress(&mut self, chunk: u64, total: Option<u64>) -> bool {
        let UpdateState::Installing { downloaded, total: size, .. } = &mut self.state else {
            return false;
        };
        *downloaded += chunk;
        if total.is_some() {
            *size = total;
        }
        let mark = match *size {
            Some(size) if size > 0 => (*downloaded).min(size) * 100 / size,
            _ => *downloaded >> 20,
        };
        if self.reported == Some(mark) {
            return false;
        }
        self.reported = Some(mark);
        true
    }

    /// The download or the installer launch failed; the same update can be tried again.
    pub fn install_failed(&mut self) {
        if let UpdateState::Installing { info, .. } = &self.state {
            let info = info.clone();
            self.state = UpdateState::Available { info };
        }
    }
}

/// Managed Tauri state.
#[derive(Default)]
pub struct UpdateService(Mutex<UpdateMachine<Update>>);

impl UpdateService {
    fn with<R>(&self, f: impl FnOnce(&mut UpdateMachine<Update>) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn state(&self) -> UpdateState {
        self.with(|m| m.state())
    }
}

pub fn current(app: &AppHandle) -> UpdateState {
    app.state::<UpdateService>().state()
}

fn publish(app: &AppHandle) -> UpdateState {
    let state = current(app);
    events::emit(app, events::UPDATE_STATE, state.clone());
    state
}

/// Runs one check, or returns the current state while a check or an install is running.
pub async fn check(app: &AppHandle) -> UpdateState {
    if !app.state::<UpdateService>().with(|m| m.begin_check()) {
        return current(app);
    }
    publish(app);
    let found = match app.updater_builder().timeout(CHECK_TIMEOUT).build() {
        Ok(updater) => updater.check().await.map_err(|err| err.to_string()),
        Err(err) => Err(err.to_string()),
    };
    match &found {
        Ok(Some(update)) => info!(version = %update.version, "update available"),
        Ok(None) => info!("the app is up to date"),
        Err(err) => warn!(error = %err, "update check failed"),
    }
    let result = found.map(|found| found.map(|update| (UpdateInfo::from(&update), update)));
    app.state::<UpdateService>().with(|m| m.finish_check(result));
    publish(app)
}

/// Downloads, verifies and starts the installer. On Windows a successful install never returns:
/// the plugin starts the installer and exits the process. No dictation can start in the meantime
/// (the exit would kill it); when the install fails, dictation works again.
pub async fn install(app: &AppHandle, controller: &ControllerHandle) -> Result<(), CommandError> {
    if cfg!(debug_assertions) {
        return Err(CommandError::new("unavailable", "only the installed app can update itself"));
    }
    // Lives until this function returns, i.e. until the app exits or the install failed.
    let hold = controller.hold_for_update().await;
    let mut update = app.state::<UpdateService>().with(|m| m.begin_install(hold.is_none()))?;
    publish(app);
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    info!(version = %update.version, "downloading the update");
    let progress_app = app.clone();
    let result = update
        .download_and_install(
            move |chunk, total| {
                if progress_app.state::<UpdateService>().with(|m| m.progress(chunk as u64, total)) {
                    publish(&progress_app);
                }
            },
            || info!("update downloaded; starting the installer"),
        )
        .await;
    if let Err(err) = result {
        warn!(error = %err, "update install failed");
        app.state::<UpdateService>().with(|m| m.install_failed());
        publish(app);
        return Err(CommandError::new("update", err.to_string()));
    }
    Ok(())
}

/// Release builds: one check after [`AUTO_CHECK_DELAY`], then every [`AUTO_CHECK_INTERVAL`],
/// each only while `ui.check_updates` is on. Failures are logged and kept as `CheckFailed`.
pub fn spawn_auto_check(app: AppHandle, settings: SettingsHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AUTO_CHECK_DELAY).await;
        loop {
            if settings.current().config.ui.check_updates {
                check(&app).await;
            }
            tokio::time::sleep(AUTO_CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(version: &str) -> UpdateInfo {
        UpdateInfo { version: version.into(), current_version: "0.1.0".into(), notes: None }
    }

    fn available() -> UpdateMachine<&'static str> {
        let mut m = UpdateMachine::default();
        assert!(m.begin_check());
        m.finish_check(Ok(Some((info("0.2.0"), "update"))));
        m
    }

    #[test]
    fn a_check_runs_once_at_a_time_and_ends_in_a_result() {
        let mut m = UpdateMachine::<&str>::default();
        assert_eq!(m.state(), UpdateState::Idle);
        assert!(m.begin_check());
        assert!(!m.begin_check(), "a second check waits for the first");
        assert_eq!(m.state(), UpdateState::Checking);
        m.finish_check(Ok(None));
        assert_eq!(m.state(), UpdateState::UpToDate);
        assert!(m.begin_check());
        m.finish_check(Ok(Some((info("0.2.0"), "update"))));
        assert_eq!(m.state(), UpdateState::Available { info: info("0.2.0") });
    }

    #[test]
    fn a_failed_check_is_kept_as_a_state_and_drops_the_old_update() {
        let mut m = available();
        assert!(m.begin_check());
        m.finish_check(Err("Could not fetch a valid release JSON from the remote".into()));
        assert_eq!(
            m.state(),
            UpdateState::CheckFailed { message: "Could not fetch a valid release JSON from the remote".into() }
        );
        assert_eq!(m.begin_install(false).unwrap_err().code, "unavailable");
    }

    #[test]
    fn install_needs_an_update_and_no_running_dictation() {
        let mut m = UpdateMachine::<&str>::default();
        assert_eq!(m.begin_install(false).unwrap_err().code, "unavailable");

        let mut m = available();
        let err = m.begin_install(true).unwrap_err();
        assert!(err.message.contains("dictation"), "{}", err.message);
        assert_eq!(m.state(), UpdateState::Available { info: info("0.2.0") }, "a refusal changes nothing");

        assert_eq!(m.begin_install(false).unwrap(), "update");
        assert_eq!(m.state(), UpdateState::Installing { info: info("0.2.0"), downloaded: 0, total: None });
        assert!(!m.begin_check(), "no check while installing");
        assert_eq!(m.begin_install(false).unwrap_err().code, "unavailable", "no second install");
    }

    #[test]
    fn progress_is_reported_once_per_whole_percent() {
        let mut m = available();
        m.begin_install(false).unwrap();
        let reports = (0..1000).filter(|_| m.progress(1, Some(1000))).count();
        assert_eq!(reports, 101, "marks 0..=100");
        assert_eq!(m.state(), UpdateState::Installing { info: info("0.2.0"), downloaded: 1000, total: Some(1000) });
    }

    #[test]
    fn progress_without_a_size_is_reported_per_mib() {
        let mut m = available();
        m.begin_install(false).unwrap();
        let reports = (0..8).filter(|_| m.progress(512 * 1024, None)).count();
        assert_eq!(reports, 5, "0.5 … 4 MiB → marks 0, 1, 2, 3, 4");
    }

    #[test]
    fn a_failed_install_can_be_retried() {
        let mut m = available();
        m.begin_install(false).unwrap();
        m.progress(10, Some(100));
        m.install_failed();
        assert_eq!(m.state(), UpdateState::Available { info: info("0.2.0") });
        assert_eq!(m.begin_install(false).unwrap(), "update");
        assert!(m.progress(1, Some(100)), "progress starts over");
    }

    #[test]
    fn progress_outside_an_install_is_ignored() {
        let mut m = available();
        assert!(!m.progress(10, Some(100)));
        assert_eq!(m.state(), UpdateState::Available { info: info("0.2.0") });
    }

    #[test]
    fn the_ui_payload_shape() {
        use serde_json::json;
        assert_eq!(serde_json::to_value(UpdateState::UpToDate).unwrap(), json!({"kind": "up_to_date"}));
        assert_eq!(
            serde_json::to_value(UpdateState::Available { info: info("0.2.0") }).unwrap(),
            json!({"kind": "available", "info": {"version": "0.2.0", "current_version": "0.1.0", "notes": null}})
        );
        let installing =
            serde_json::to_value(UpdateState::Installing { info: info("0.2.0"), downloaded: 5, total: None }).unwrap();
        assert_eq!(installing["kind"], "installing");
        assert_eq!(installing["downloaded"], 5);
        assert_eq!(installing["total"], serde_json::Value::Null);
        assert_eq!(
            serde_json::to_value(UpdateState::CheckFailed { message: "x".into() }).unwrap(),
            json!({"kind": "check_failed", "message": "x"})
        );
    }
}
