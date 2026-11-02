fn main() {
    // Ordinary debug tests do not need a bundled CLI. Tauri's build hook stages
    // the release CLI before packaging; an empty resource directory is valid.
    let cli_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/cli-bundle");
    std::fs::create_dir_all(&cli_dir).expect("create CLI resource staging directory");
    if std::env::var("PROFILE").as_deref() == Ok("release")
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
    {
        assert!(cli_dir.join("opit.exe").is_file(), "Run node scripts/build-cli.mjs before a release app build");
    }
    // Integration tests that construct a Tauri mock app link menu APIs from Common Controls 6.
    // The app's manifest is embedded by tauri-build, but Cargo's separate test executables need one too.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=windows/tests.manifest");
        let manifest =
            std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows/tests.manifest");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::build();
}
