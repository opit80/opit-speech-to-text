//! Session policy. A Wayland session must never use XWayland for foreground discovery.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionKind {
    Wayland,
    X11,
    Unknown,
}

pub(crate) fn classify(kind: Option<&str>, wayland: bool, display: bool) -> SessionKind {
    if kind == Some("wayland") || wayland {
        SessionKind::Wayland
    } else if (kind == Some("x11") || kind.is_none()) && display {
        SessionKind::X11
    } else {
        SessionKind::Unknown
    }
}

pub(crate) fn current() -> SessionKind {
    classify(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    )
}

/// A tray icon object alone does not imply that the desktop displays it.
pub(crate) async fn tray_host_available() -> bool {
    let probe = async {
        let connection = ashpd::zbus::Connection::session().await.ok()?;
        let watcher = ashpd::zbus::Proxy::new(
            &connection,
            "org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            "org.kde.StatusNotifierWatcher",
        )
        .await
        .ok()?;
        watcher.get_property::<bool>("IsStatusNotifierHostRegistered").await.ok()
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), probe).await.ok().flatten().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wayland_does_not_become_x11_when_xwayland_display_exists() {
        assert_eq!(classify(Some("wayland"), true, true), SessionKind::Wayland);
        assert_eq!(classify(Some("wayland"), false, true), SessionKind::Wayland);
        assert_eq!(classify(None, true, true), SessionKind::Wayland);
        assert_eq!(classify(Some("x11"), false, true), SessionKind::X11);
        assert_eq!(classify(None, false, true), SessionKind::X11);
        assert_eq!(classify(Some("tty"), false, true), SessionKind::Unknown);
        assert_eq!(classify(None, false, false), SessionKind::Unknown);
    }
}
