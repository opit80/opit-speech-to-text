//! All ashpd users share this process-lived Tokio runtime and host registration.
//!
//! ashpd 0.13 caches its connection globally; zbus's Tokio driver belongs to the
//! runtime that first opened it. Never open that connection on a temporary runtime.

use std::future::Future;
use std::sync::OnceLock;

pub(super) const APP_ID: &str = "io.github.opit80.opit-speech-to-text";

// reis contains thread-local callbacks, so portal clients run on one LocalSet.
// Jobs cross threads as factories; the !Send futures are created on the owner thread.
type Job = Box<dyn FnOnce() + Send>;

fn start_worker() -> Result<tokio::sync::mpsc::UnboundedSender<Job>, std::io::Error> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
    std::thread::Builder::new().name("opit-portals".into()).spawn(move || {
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async move {
            while let Some(job) = rx.recv().await {
                job();
            }
        }));
    })?;
    Ok(tx)
}

pub(super) fn spawn<F, Fut>(start: F) -> Result<(), &'static str>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + 'static,
{
    static WORKER: OnceLock<Result<tokio::sync::mpsc::UnboundedSender<Job>, std::io::Error>> = OnceLock::new();
    let tx = WORKER.get_or_init(start_worker).as_ref().map_err(|_| "Portal runtime could not start")?;
    tx.send(Box::new(move || {
        tokio::task::spawn_local(start());
    }))
    .map_err(|_| "Portal runtime stopped")
}

#[derive(Default)]
struct RegistrationOnce(tokio::sync::Mutex<bool>);

impl RegistrationOnce {
    async fn run<F, Fut>(&self, register: F) -> Result<(), &'static str>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), &'static str>>,
    {
        let mut registered = self.0.lock().await;
        if !*registered {
            register().await?;
            *registered = true;
        }
        Ok(())
    }
}

/// An AppImage has no package installing the host identity required by RemoteDesktop.
async fn ensure_appimage_host_entry() -> Result<(), &'static str> {
    let Some(image) = std::env::var_os("APPIMAGE") else {
        return Ok(());
    };
    tokio::task::spawn_blocking(move || {
        let image = std::path::PathBuf::from(image);
        if !image.is_absolute() { return Err("Invalid AppImage path"); }
        let exec = super::shortcuts::desktop_exec_arg(image.to_str().ok_or("Invalid AppImage path")?)?;
        let dir = dirs::data_dir().ok_or("Desktop applications directory is unavailable")?.join("applications");
        std::fs::create_dir_all(&dir).map_err(|_| "Could not create host application directory")?;
        std::fs::write(dir.join(format!("{APP_ID}.desktop")), format!(
            "[Desktop Entry]\nType=Application\nName=Opit Speech to Text\nExec={exec}\nIcon=opit-speech-to-text\nTerminal=false\nCategories=Utility;Audio;\n"
        )).map_err(|_| "Could not register the host application")
    }).await.map_err(|_| "Host application worker stopped")?
}

pub(super) async fn register_host_app() -> Result<(), &'static str> {
    static REGISTRATION: RegistrationOnce = RegistrationOnce(tokio::sync::Mutex::const_new(false));
    // Detach on caller cancellation: a successful D-Bus Register must still be
    // remembered, even if the RemoteDesktop caller was cancelled while it ran.
    tokio::spawn(async {
        REGISTRATION
            .run(|| async {
                ensure_appimage_host_entry().await?;
                let id = APP_ID.parse().map_err(|_| "Invalid app ID")?;
                ashpd::register_host_app(id)
                    .await
                    .map_err(|_| "Install the app desktop entry before enabling RemoteDesktop paste")
            })
            .await
    })
    .await
    .map_err(|_| "Portal registration worker stopped")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn portal_runtime_drives_local_jobs_after_the_first_attempt_exits() {
        let mut owner = None;
        for _ in 0..2 {
            let (tx, rx) = std::sync::mpsc::channel();
            spawn(move || async move {
                // A thread-local reis-style value must survive across an await.
                let local = std::rc::Rc::new(7);
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                assert_eq!(*local, 7);
                tx.send(std::thread::current().id()).unwrap();
            })
            .unwrap();
            let thread = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if let Some(owner) = owner {
                assert_eq!(thread, owner);
            }
            owner = Some(thread);
        }
    }

    #[tokio::test]
    async fn abandoned_waiter_does_not_cancel_successful_registration() {
        let registration = Arc::new(RegistrationOnce::default());
        let (finish, finish_rx) = tokio::sync::oneshot::channel();
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let (done, done_rx) = tokio::sync::oneshot::channel();
        let state = registration.clone();
        let task = tokio::spawn(async move {
            state
                .run(|| async {
                    let _ = started.send(());
                    finish_rx.await.unwrap();
                    Ok(())
                })
                .await
                .unwrap();
            let _ = done.send(());
        });
        started_rx.await.unwrap();
        drop(task); // Same detach semantics used by register_host_app on cancellation.
        finish.send(()).unwrap();
        done_rx.await.unwrap();
        registration.run(|| async { panic!("must remember a late success") }).await.unwrap();
    }

    #[tokio::test]
    async fn host_registration_failure_is_retryable_and_success_is_cached() {
        let registration = RegistrationOnce::default();
        assert!(registration.run(|| async { Err("desktop entry missing") }).await.is_err());
        assert!(registration.run(|| async { Ok(()) }).await.is_ok());
        assert!(registration.run(|| async { panic!("must not register twice") }).await.is_ok());
    }

    #[tokio::test]
    async fn concurrent_host_registration_calls_register_only_once() {
        let registration = Arc::new(RegistrationOnce::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let registration = registration.clone();
            let calls = calls.clone();
            tasks.push(tokio::spawn(async move {
                registration
                    .run(|| async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::task::yield_now().await;
                        Ok(())
                    })
                    .await
            }));
        }
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
