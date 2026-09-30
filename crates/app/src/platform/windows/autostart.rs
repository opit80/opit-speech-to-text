//! Start-with-Windows via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

use windows_registry::CURRENT_USER;

use crate::platform::Autostart;

/// Per-user Run key.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Value name under the Run key.
pub const VALUE_NAME: &str = "Opit Speech to Text";
/// Command-line flag passed when Windows launches the app at sign-in.
pub const AUTOSTART_FLAG: &str = "--autostart";

/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`: the key or value does not exist.
const NOT_FOUND: i32 = 0x8007_0002_u32 as i32;

/// [`Autostart`] backed by a string value under an HKCU key.
#[derive(Debug, Clone)]
pub struct RegistryAutostart {
    key_path: String,
    value_name: String,
    command: String,
}

impl RegistryAutostart {
    /// Key path is relative to HKCU.
    pub fn new(key_path: impl Into<String>, value_name: impl Into<String>, command: impl Into<String>) -> Self {
        Self { key_path: key_path.into(), value_name: value_name.into(), command: command.into() }
    }

    /// The real Run entry: `"<current exe>" --autostart`.
    pub fn for_current_exe() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("could not locate the executable: {e}"))?;
        Ok(Self::new(RUN_KEY, VALUE_NAME, format!("\"{}\" {AUTOSTART_FLAG}", exe.display())))
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}

impl Autostart for RegistryAutostart {
    /// True when the value exists, even if it points at an old exe path.
    fn is_enabled(&self) -> Result<bool, String> {
        let key = match CURRENT_USER.open(&self.key_path) {
            Ok(key) => key,
            Err(e) if e.code().0 == NOT_FOUND => return Ok(false),
            Err(e) => return Err(format!("could not read the startup setting: {e}")),
        };
        match key.get_type(&self.value_name) {
            Ok(_) => Ok(true),
            Err(e) if e.code().0 == NOT_FOUND => Ok(false),
            Err(e) => Err(format!("could not read the startup setting: {e}")),
        }
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        if enabled {
            let key = CURRENT_USER
                .create(&self.key_path)
                .map_err(|e| format!("could not open the startup registry key: {e}"))?;
            return key
                .set_string(&self.value_name, &self.command)
                .map_err(|e| format!("could not enable start with Windows: {e}"));
        }
        let key = match CURRENT_USER.options().read().write().open(&self.key_path) {
            Ok(key) => key,
            Err(e) if e.code().0 == NOT_FOUND => return Ok(()),
            Err(e) => return Err(format!("could not open the startup registry key: {e}")),
        };
        match key.remove_value(&self.value_name) {
            Ok(()) => Ok(()),
            Err(e) if e.code().0 == NOT_FOUND => Ok(()),
            Err(e) => Err(format!("could not disable start with Windows: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = r"Software\opit-speech-to-text-test";

    #[test]
    fn current_exe_command_is_quoted_with_flag() {
        let autostart = RegistryAutostart::for_current_exe().unwrap();
        assert!(autostart.command().starts_with('"'));
        assert!(autostart.command().ends_with("\" --autostart"));
    }

    #[test]
    #[ignore = "writes to HKCU in the real registry"]
    fn enable_disable_round_trip() {
        let _ = CURRENT_USER.remove_tree(TEST_KEY);
        let command = r#""C:\Program Files\Opit\opit.exe" --autostart"#;
        let autostart = RegistryAutostart::new(TEST_KEY, VALUE_NAME, command);

        assert!(!autostart.is_enabled().unwrap(), "missing key reads as disabled");
        autostart.set_enabled(false).expect("disabling with no key is ok");

        autostart.set_enabled(true).unwrap();
        assert!(autostart.is_enabled().unwrap());
        assert_eq!(CURRENT_USER.open(TEST_KEY).unwrap().get_string(VALUE_NAME).unwrap(), command);

        let moved = RegistryAutostart::new(TEST_KEY, VALUE_NAME, r#""D:\opit.exe" --autostart"#);
        moved.set_enabled(true).expect("enabling again rewrites the command");
        assert_eq!(CURRENT_USER.open(TEST_KEY).unwrap().get_string(VALUE_NAME).unwrap(), moved.command());

        autostart.set_enabled(false).unwrap();
        assert!(!autostart.is_enabled().unwrap());
        autostart.set_enabled(false).expect("disabling twice is ok");

        CURRENT_USER.remove_tree(TEST_KEY).unwrap();
        assert!(CURRENT_USER.open(TEST_KEY).is_err());
    }
}
