//! Global subscriber test: its own process, so `init` can run exactly once.

use std::fs;

use opit_app::logging;

#[test]
fn init_writes_daily_file_without_ansi_and_logs_panic_location() {
    let dir = tempfile::tempdir().unwrap();
    // Eight stale files: startup pruning leaves 6, then today's file makes 7.
    for day in 1..=8 {
        fs::write(dir.path().join(format!("opit.2020-01-0{day}.log")), "old\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20)); // distinct creation times
    }
    fs::write(dir.path().join("keep-me.txt"), "not a log").unwrap();

    let guard = logging::init(dir.path()).expect("init");
    assert!(logging::init(dir.path()).is_err(), "second init must fail");

    tracing::info!(provider = "openai", "dictation finished");
    tracing::debug!("hidden at the default level");
    let result = std::panic::catch_unwind(|| panic!("secret transcript text"));
    assert!(result.is_err());
    drop(guard); // flush the non-blocking writer

    let mut names: Vec<String> =
        fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    println!("files: {names:?}");
    let today = names.iter().find(|n| !n.starts_with("opit.2020") && n.starts_with("opit.")).expect("today's file");
    assert!(today.len() == "opit.YYYY-MM-DD.log".len() && today.ends_with(".log"), "{today}");
    assert_eq!(names.iter().filter(|n| n.starts_with("opit.")).count(), 7, "{names:?}");
    assert!(names.contains(&"keep-me.txt".to_owned()));

    let text = fs::read_to_string(dir.path().join(today)).unwrap();
    println!("{text}");
    assert!(text.contains("dictation finished") && text.contains("provider=\"openai\""));
    assert!(!text.contains("hidden at the default level"));
    assert!(!text.contains('\u{1b}'), "no ANSI escapes in files");
    assert!(text.contains("panic") && text.contains("logging_init.rs"), "panic location logged");
    assert!(!text.contains("secret transcript text"), "panic payload must not be logged");
}
