//! Windows backend construction and the existing startup fallbacks.

use std::sync::Arc;

use opit_core::config::AppConfig;
use tracing::error;

use super::{CpalMicrophone, KeyringStore, RegistryAutostart, WinHotkey, WinOverlay, WinPaster, WinSounds};
use crate::app_core::Platform;
use crate::platform::factory::PlatformServices;
use crate::platform::{
    Autostart, DebugAutostart, NoAutostart, NoOverlay, Overlay, OverlaySink, SecretStore, UnavailableSecrets,
};

pub(crate) fn create(config: &AppConfig, overlay_sink: OverlaySink) -> PlatformServices {
    let overlay: Arc<dyn Overlay> = match WinOverlay::new(config.ui.overlay_position, overlay_sink) {
        Ok(overlay) => Arc::new(overlay),
        Err(err) => {
            error!(error = %err, "overlay unavailable");
            Arc::new(NoOverlay)
        }
    };
    let secrets: Arc<dyn SecretStore> = match KeyringStore::new() {
        Ok(store) => Arc::new(store),
        Err(err) => {
            error!(error = %err, "credential store unavailable");
            Arc::new(UnavailableSecrets(err.to_string()))
        }
    };
    // Debug builds never touch the Run key, whichever code path asks for it.
    let autostart: Arc<dyn Autostart> = if cfg!(debug_assertions) {
        Arc::new(DebugAutostart)
    } else {
        match RegistryAutostart::for_current_exe() {
            Ok(autostart) => Arc::new(autostart),
            Err(err) => Arc::new(NoAutostart(err)),
        }
    };
    let hotkey = Arc::new(WinHotkey::new());
    let mic = Arc::new(CpalMicrophone::new());
    let foreground = Arc::new(crate::platform::windows::foreground::WinForegroundApp);
    let media = Arc::new(crate::platform::windows::media::new());
    let sounds = Arc::new(WinSounds::new());
    sounds.warm_up();
    PlatformServices {
        platform: Platform { foreground, media, secrets, hotkey, mic, overlay, autostart },
        paster: Arc::new(WinPaster::new()),
        sounds,
    }
}
