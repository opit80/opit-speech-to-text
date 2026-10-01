//! The installer, the uninstaller hooks and the app must agree on names and paths.

use opit_app::platform::windows::autostart::VALUE_NAME;

const CONF: &str = include_str!("../tauri.conf.json");
const HOOKS: &str = include_str!("../windows/hooks.nsh");

fn conf() -> serde_json::Value {
    serde_json::from_str(CONF).expect("tauri.conf.json is valid JSON")
}

#[test]
fn the_uninstaller_removes_the_run_value_the_app_writes() {
    // Tauri's uninstaller deletes HKCU\...\Run\<productName> (outside update mode).
    assert_eq!(conf()["productName"], VALUE_NAME);
}

#[test]
fn the_installer_is_per_user_in_english_and_turkish() {
    let conf = conf();
    let nsis = &conf["bundle"]["windows"]["nsis"];
    assert_eq!(conf["bundle"]["targets"], serde_json::json!(["nsis"]));
    assert_eq!(nsis["installMode"], "currentUser");
    assert_eq!(nsis["languages"], serde_json::json!(["English", "Turkish"]));
    assert_eq!(nsis["installerHooks"], "windows/hooks.nsh");
    assert_eq!(conf["bundle"]["publisher"], "opit80");
}

#[test]
fn destructive_uninstall_hooks_need_the_checkbox_and_skip_updates() {
    let mut destructive = 0;
    for block in HOOKS.split("!macro ").skip(1) {
        let body = block.split("!macroend").next().unwrap();
        if ["RmDir", "ExecWait", "DeleteRegValue", "DeleteRegKey"].iter().any(|cmd| body.contains(cmd)) {
            destructive += 1;
            assert!(body.contains("${If} $DeleteAppDataCheckboxState = 1"), "unguarded hook: {block}");
            assert!(body.contains("${AndIf} $UpdateMode <> 1"), "hook runs during updates: {block}");
        }
    }
    assert_eq!(destructive, 2, "PREUNINSTALL (credentials) and POSTUNINSTALL (data folder)");
}

#[test]
fn the_hooks_target_the_apps_data_folder_and_cleanup_flag() {
    // RmDir /r "$APPDATA\opit-speech-to-text"
    let data = format!("RmDir /r \"$APPDATA\\{}\"", opit_core::config::APP_DIR_NAME);
    // "$INSTDIR\${MAINBINARYNAME}.exe" --delete-credentials
    let cleanup = format!("\"$INSTDIR\\${{MAINBINARYNAME}}.exe\" {}", opit_app::uninstall::DELETE_CREDENTIALS_FLAG);
    assert!(HOOKS.contains(&data), "missing: {data}");
    assert!(HOOKS.contains(&cleanup), "missing: {cleanup}");
}
