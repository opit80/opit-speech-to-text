//! Opt-in verification of the published GitHub endpoint and its actual signed installer.
//! Does not install anything or touch the user's running application/data/credentials.

use std::path::PathBuf;
use std::time::Duration;

use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;

#[tokio::test]
#[ignore = "requires a published release and OPIT_RELEASE_ASSETS containing its downloaded assets"]
async fn published_updater_matches_verified_release_and_respects_installed_version() {
    let dir = PathBuf::from(std::env::var("OPIT_RELEASE_ASSETS").expect("set the downloaded release assets directory"));
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("latest.json")).unwrap()).unwrap();
    let version = manifest["version"].as_str().unwrap();
    let expected = std::fs::read(dir.join(format!("opit-speech-to-text_{version}_x64-setup.exe"))).unwrap();
    let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();

    for current in ["0.1.2", version] {
        let mut context = mock_context(noop_assets());
        context.package_info_mut().version = current.parse().unwrap();
        context.config_mut().plugins.0.insert("updater".into(), conf["plugins"]["updater"].clone());
        let app = mock_builder().plugin(tauri_plugin_updater::Builder::new().build()).build(context).unwrap();
        let updater = app.updater_builder().timeout(Duration::from_secs(60)).build().unwrap();
        let found = updater.check().await.expect("published manifest is available over HTTPS");
        if current == version {
            assert!(found.is_none(), "the installed latest version must not be offered the same update");
        } else {
            let update = found.expect("the older installed version must discover the published release");
            assert_eq!(update.current_version, current);
            assert_eq!(update.version, version);
            let downloaded = update.download(|_, _| {}, || {}).await.expect("published installer verifies");
            assert_eq!(downloaded, expected, "published download must match the independently verified assets");
        }
    }
}
