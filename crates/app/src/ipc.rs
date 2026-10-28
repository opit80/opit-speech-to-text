//! Local control dispatcher. Blocking app APIs stay on workers; controller
//! mutations are acknowledged by the actor that owns the dictation queue.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use opit_ipc::{Command, Error, Format, Request};
use serde::Serialize;
use serde_json::{Value, json};

use crate::app_core::{AppCore, CommandError};
use crate::controller::{ControlAction, ErrorKind};

fn safe_error(error: CommandError) -> Error {
    // Config/SQLite/platform errors may contain user input. Only forward codes.
    Error::new(error.code, "The app could not complete the command.")
}

fn controller_error(kind: ErrorKind) -> Error {
    let code =
        serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or("unavailable".into());
    Error::new(&code, "The dictation command could not be completed in the current state.")
}

fn value<T: Serialize>(result: T) -> Result<Value, Error> {
    serde_json::to_value(result).map_err(|_| Error::new("protocol", "Could not encode command result."))
}

async fn blocking<T: Send + 'static>(
    core: Arc<AppCore>,
    work: impl FnOnce(&AppCore) -> Result<T, CommandError> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(move || work(&core))
        .await
        .map_err(|_| Error::new("unavailable", "The app worker stopped."))?
        .map_err(safe_error)
}

pub async fn dispatch(core: Arc<AppCore>, request: Request) -> Result<Value, Error> {
    request.validate()?;
    let action = match request.command {
        Command::Toggle => Some(ControlAction::Toggle),
        Command::Start => Some(ControlAction::Start),
        Command::Stop => Some(ControlAction::Stop),
        Command::Cancel => Some(ControlAction::Cancel),
        Command::Pause => Some(ControlAction::Pause),
        Command::Resume => Some(ControlAction::Resume),
        Command::Retry => Some(ControlAction::Retry),
        _ => None,
    };
    if let Some(action) = action {
        return value(core.controller.control(action).await.map_err(controller_error)?);
    }
    match request.command {
        Command::Ask { text } => {
            let text = core.controller.ask(text).await.map_err(controller_error)?;
            Ok(json!({"text": text}))
        }
        Command::Status => value(core.status()),
        Command::Version => Ok(json!({"version":env!("CARGO_PKG_VERSION")})),
        Command::Record { seconds } => {
            let text = core.controller.record(seconds).await.map_err(controller_error)?;
            Ok(json!({"text":text}))
        }
        Command::HistoryList { limit, search } => value(
            blocking(core, move |core| match search {
                Some(search) => core.history_search(&search, limit),
                None => core.history_recent(limit, None),
            })
            .await?,
        ),
        Command::HistoryRetry { id } => {
            let audio = blocking(core.clone(), move |core| core.history_retry_audio(id)).await?;
            core.controller.retry_history(id, audio).await.map_err(controller_error)?;
            Ok(json!({"id":id}))
        }
        Command::ProfilesList => {
            let config = core.config();
            Ok(Value::Array(
                config
                    .profiles
                    .iter()
                    .map(|p| {
                        json!({
                            "id":p.id,"name":p.name,"active":p.id == config.active_profile_id
                        })
                    })
                    .collect(),
            ))
        }
        Command::ProfileUse { id } => {
            let config = blocking(core, move |core| core.set_active_profile(&id)).await?;
            Ok(json!({"active_profile_id":config.active_profile_id}))
        }
        Command::Transcribe { path, format, timestamps, cleanup } => {
            let work =
                core.controller.begin_file_work().ok_or_else(|| Error::new("file_busy", "An update is installing."))?;
            let runtime = tokio::runtime::Handle::current();
            let result = blocking(core, move |core| {
                let _work = work;
                crate::file_jobs::transcribe_path(
                    core,
                    core.settings.current(),
                    &crate::file_jobs::FileOptions { cleanup, ..Default::default() },
                    std::path::Path::new(&path),
                    &AtomicBool::new(false),
                    |_| {},
                    &runtime,
                )
            })
            .await?;
            if (timestamps || matches!(format, Format::Srt)) && !result.timestamps_available {
                return Err(Error::new("file_timestamps", "The provider did not return usable timestamps."));
            }
            let text = match format {
                Format::Txt => opit_core::files::render_txt(&result, timestamps, false),
                Format::Srt => opit_core::files::render_srt(&result, false)
                    .ok_or_else(|| Error::new("file_timestamps", "The provider did not return usable timestamps."))?,
            };
            Ok(json!({"text":text,"cleanup_failed":result.cleanup_failed,"duration_ms":result.duration_ms}))
        }
        _ => Err(Error::new("invalid_request", "Unknown command.")),
    }
}

#[cfg(windows)]
pub fn spawn(app: tauri::AppHandle, core: Arc<AppCore>) {
    use opit_ipc::windows::{create_server, pipe_name};
    use opit_ipc::{MAX_RESPONSE, Response, write_frame};
    use std::time::Duration;

    tauri::async_runtime::spawn(async move {
        let Ok(name) = pipe_name() else {
            tracing::warn!("local control identity unavailable");
            return;
        };
        let Ok(mut server) = create_server(&name, true) else {
            tracing::warn!("local control pipe unavailable");
            return;
        };
        let capacity = Arc::new(tokio::sync::Semaphore::new(8));
        let file_capacity = Arc::new(tokio::sync::Semaphore::new(1));
        loop {
            let Ok(permit) = capacity.clone().acquire_owned().await else {
                break;
            };
            if server.connect().await.is_err() {
                break;
            }
            let connected = server;
            // Recreate before handing off so another process cannot occupy the name.
            let Ok(next) = create_server(&name, false) else {
                break;
            };
            server = next;
            let core = core.clone();
            let app = app.clone();
            let file_capacity = file_capacity.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let mut connected = connected;
                let result = connection_request(&mut connected, &app, core, file_capacity).await;
                let response = Response::from_result(result);
                let write = async {
                    if let Err(error) = write_frame(&mut connected, &response, MAX_RESPONSE).await
                        && error.code == "response_limit"
                    {
                        let _ = write_frame(&mut connected, &Response::from_result(Err(error)), MAX_RESPONSE).await;
                    }
                };
                let _ = tokio::time::timeout(Duration::from_secs(5), write).await;
            });
        }
        tracing::warn!("local control server stopped");
    });
}

#[cfg(windows)]
async fn connection_request(
    connected: &mut tokio::net::windows::named_pipe::NamedPipeServer,
    app: &tauri::AppHandle,
    core: Arc<AppCore>,
    file_capacity: Arc<tokio::sync::Semaphore>,
) -> Result<Value, Error> {
    use opit_ipc::{MAX_REQUEST, decode_request, read_frame};
    use tokio::io::BufReader;
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        read_frame(&mut BufReader::new(connected), MAX_REQUEST),
    )
    .await
    .map_err(|_| Error::new("timeout", "Local request read timed out."))??;
    let request = decode_request(&bytes)?;
    let profile_change = matches!(request.command, Command::ProfileUse { .. });
    let _file_permit = if matches!(request.command, Command::Transcribe { .. }) {
        Some(
            file_capacity
                .try_acquire_owned()
                .map_err(|_| Error::new("file_busy", "A CLI file job is already running."))?,
        )
    } else {
        None
    };
    let result = dispatch(core.clone(), request).await;
    if profile_change && result.is_ok() {
        crate::events::config_changed(app, &core.config());
    }
    result
}
