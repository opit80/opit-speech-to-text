//! File logging: daily-rotated `opit.YYYY-MM-DD.log` (UTC date), last 7 files kept.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

/// Log file name prefix.
pub const FILE_PREFIX: &str = "opit";
/// Log file name suffix.
pub const FILE_SUFFIX: &str = "log";
/// Rotated files kept, today's included.
pub const MAX_FILES: usize = 7;

#[derive(Debug, thiserror::Error)]
pub enum LogInitError {
    #[error("could not create the log folder: {0}")]
    Dir(#[from] std::io::Error),
    #[error("could not open the log file: {0}")]
    Appender(#[from] tracing_appender::rolling::InitError),
    #[error("a global logger is already installed: {0}")]
    Subscriber(String),
}

/// Installs the global subscriber and the panic hook. Keep the guard alive until exit;
/// dropping it flushes the background writer.
pub fn init(log_dir: &Path) -> Result<WorkerGuard, LogInitError> {
    // Checked first: building the appender already prunes old files and opens today's file.
    if tracing::dispatcher::has_been_set() {
        return Err(LogInitError::Subscriber("init was called twice".into()));
    }
    std::fs::create_dir_all(log_dir)?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(FILE_PREFIX)
        .filename_suffix(FILE_SUFFIX)
        .max_log_files(MAX_FILES)
        .build(log_dir)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    // RUST_LOG overrides; invalid directives are ignored rather than fatal.
    let filter = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into()).from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .map_err(|e| LogInitError::Subscriber(e.to_string()))?;
    install_panic_hook();
    Ok(guard)
}

/// Logs the panic location (never the payload: panic messages such as string-slicing errors can
/// quote user text), then runs the previous hook.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed");
        match info.location() {
            Some(loc) => tracing::error!(thread, file = loc.file(), line = loc.line(), "panic"),
            None => tracing::error!(thread, "panic at an unknown location"),
        }
        previous(info);
    }));
}
