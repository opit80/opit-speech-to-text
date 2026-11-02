//! Per-user XDG autostart entry. Only enabled by the app's saved settings.

use std::path::PathBuf;

use crate::platform::Autostart;

const ENTRY_NAME: &str = "io.github.opit80.opit-speech-to-text.desktop";

pub struct DesktopAutostart {
    path: PathBuf,
    exe: PathBuf,
}

impl DesktopAutostart {
    pub fn for_current_exe() -> Result<Self, String> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
            .ok_or("Could not locate the configuration directory")?;
        let exe = std::env::current_exe().map_err(|_| "Could not locate the executable")?;
        let exe = launch_path(exe, std::env::var_os("APPIMAGE").as_deref().map(std::path::Path::new))?;
        Ok(Self { path: config.join("autostart").join(ENTRY_NAME), exe })
    }

    fn entry(&self) -> String {
        let escaped = self.exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            "[Desktop Entry]\nType=Application\nName=Opit Speech to Text\nExec=\"{escaped}\" --autostart\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        )
    }
}

fn launch_path(exe: PathBuf, image: Option<&std::path::Path>) -> Result<PathBuf, String> {
    match image {
        Some(path) if path.is_absolute() => Ok(path.to_owned()),
        Some(_) => Err("The AppImage launch path must be absolute".into()),
        None => Ok(exe),
    }
}

impl Autostart for DesktopAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        Ok(self.path.exists())
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        if !enabled {
            return match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err("Could not remove the autostart entry".into()),
            };
        }
        let parent = self.path.parent().ok_or("Invalid autostart path")?;
        std::fs::create_dir_all(parent).map_err(|_| "Could not create the autostart directory")?;
        std::fs::write(&self.path, self.entry()).map_err(|_| "Could not write the autostart entry".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_uses_current_executable_and_existing_flag() {
        let entry = DesktopAutostart { path: PathBuf::from("unused"), exe: PathBuf::from("/tmp/Opit App") };
        assert!(entry.entry().contains("Exec=\"/tmp/Opit App\" --autostart"));
    }

    #[test]
    fn appimage_autostart_uses_the_persistent_image_instead_of_its_mount() {
        let mounted = PathBuf::from("/tmp/.mount_opit/usr/bin/opit-speech-to-text");
        let image = std::path::Path::new("/home/test/Opit.AppImage");
        assert_eq!(launch_path(mounted.clone(), Some(image)).unwrap(), image);
        assert_eq!(launch_path(mounted.clone(), None).unwrap(), mounted);
        assert!(launch_path(mounted, Some(std::path::Path::new("relative.AppImage"))).is_err());
    }
}
