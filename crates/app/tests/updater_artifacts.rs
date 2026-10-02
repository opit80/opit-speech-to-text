//! Exercises the real updater's network and signature path without launching an installer.
//! Run after packaging with OPIT_RELEASE_ASSETS pointing to the verified release directory.

use std::path::PathBuf;
use std::time::Duration;

use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
#[ignore = "requires OPIT_RELEASE_ASSETS; downloads on loopback but never installs"]
async fn real_updater_downloads_verified_artifacts_and_refuses_tampering() {
    let dir = PathBuf::from(std::env::var("OPIT_RELEASE_ASSETS").expect("set OPIT_RELEASE_ASSETS after packaging"));
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("latest.json")).unwrap()).unwrap();
    let version = manifest["version"].as_str().unwrap().to_owned();
    let bytes = std::fs::read(dir.join(format!("opit-speech-to-text_{version}_x64-setup.exe"))).unwrap();
    let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let mut context = mock_context(noop_assets());
    context.package_info_mut().version = "0.1.2".parse().unwrap();
    context.config_mut().plugins.0.insert("updater".into(), conf["plugins"]["updater"].clone());
    let app = mock_builder().plugin(tauri_plugin_updater::Builder::new().build()).build(context).unwrap();
    let server = MockServer::start().await;
    manifest["platforms"]["windows-x86_64"]["url"] = format!("{}/installer", server.uri()).into();

    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/installer"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.clone()))
        .mount(&server)
        .await;
    let updater = app
        .updater_builder()
        .target("windows-x86_64")
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .endpoints(vec![format!("{}/latest.json", server.uri()).parse().unwrap()])
        .unwrap()
        .build()
        .unwrap();
    let update = updater.check().await.unwrap().expect("candidate is newer than installed app 0.1.2");
    assert_eq!(update.current_version, "0.1.2");
    assert_eq!(update.version, version);
    let downloaded = update.download(|_, _| {}, || {}).await.unwrap();
    assert_eq!(downloaded, bytes);

    server.reset().await;
    let mut corrupt = bytes;
    corrupt[0] ^= 1;
    Mock::given(method("GET"))
        .and(path("/installer"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(corrupt))
        .mount(&server)
        .await;
    assert!(update.download(|_, _| {}, || {}).await.is_err(), "changed installer must never verify");

    server.reset().await;
    manifest["version"] = "99.0.0".into();
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/installer"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(downloaded))
        .mount(&server)
        .await;
    let forged = updater.check().await.unwrap().unwrap();
    assert!(forged.download(|_, _| {}, || {}).await.is_err(), "manifest version must match the signed version");

    // The default comparator must not offer the same version or a downgrade.
    for current_or_older in ["0.1.2", "0.1.1"] {
        server.reset().await;
        manifest["version"] = current_or_older.into();
        Mock::given(method("GET"))
            .and(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
            .mount(&server)
            .await;
        assert!(updater.check().await.unwrap().is_none());
    }
}
