//! Startup backend bundle. Platform policy stays with the app's startup code.

use std::sync::Arc;

#[cfg(windows)]
use opit_core::config::AppConfig;

#[cfg(windows)]
use super::OverlaySink;
use super::{Paster, Sounds};
use crate::app_core::Platform;

pub(crate) struct PlatformServices {
    pub platform: Platform,
    pub paster: Arc<dyn Paster>,
    pub sounds: Arc<dyn Sounds>,
}

#[cfg(windows)]
pub(crate) fn create(config: &AppConfig, overlay_sink: OverlaySink) -> PlatformServices {
    super::windows::factory::create(config, overlay_sink)
}
