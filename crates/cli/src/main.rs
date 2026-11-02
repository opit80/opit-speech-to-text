//! Console entry point for Opit Speech to Text. No app, keyring or Tauri dependency.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use opit_ipc::{Command, Error, Format, Outcome, Request, Response};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "opit", version, about = "Control Opit Speech to Text in the current user session")]
struct Args {
    /// Print a versioned JSON response (including errors).
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
    /// Ask the configured agent; omit --text to record until opit stop.
    Ask {
        #[arg(long)]
        text: Option<String>,
    },
    Status,
    Toggle,
    Start,
    Stop,
    Cancel,
    Pause,
    Resume,
    /// Retry the last failed in-memory dictation.
    Retry,
    /// Record a timed microphone clip and print the final text. Never changes the clipboard or pastes.
    Record {
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=300))]
        seconds: u32,
        /// Explicitly retain the safe default: no paste and no clipboard changes.
        #[arg(long)]
        no_paste: bool,
    },
    History {
        #[command(subcommand)]
        command: History,
    },
    /// Transcribe a media file using the app's active profile and stored credentials.
    Transcribe {
        file: PathBuf,
        #[arg(long, conflicts_with = "txt")]
        srt: bool,
        #[arg(long)]
        txt: bool,
        /// Prefix text segments with timestamps. SRT always requires real timestamps.
        #[arg(long)]
        timestamps: bool,
        #[arg(long)]
        cleanup: bool,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    Profiles {
        #[command(subcommand)]
        command: Profiles,
    },
    Profile {
        #[command(subcommand)]
        command: Profile,
    },
    /// Print this CLI's version without requiring a running app.
    Version,
}

#[derive(Subcommand)]
enum History {
    List {
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
        #[arg(long)]
        search: Option<String>,
    },
    Retry {
        #[arg(value_parser = clap::value_parser!(i64).range(1..))]
        id: i64,
    },
}

#[derive(Subcommand)]
enum Profiles {
    List,
}

#[derive(Subcommand)]
enum Profile {
    Use { id: String },
}

impl Args {
    fn request(&self) -> Result<Request, Error> {
        let command = match &self.verb {
            Verb::Ask { text } => Command::Ask { text: text.clone() },
            Verb::Status => Command::Status,
            Verb::Toggle => Command::Toggle,
            Verb::Start => Command::Start,
            Verb::Stop => Command::Stop,
            Verb::Cancel => Command::Cancel,
            Verb::Pause => Command::Pause,
            Verb::Resume => Command::Resume,
            Verb::Retry => Command::Retry,
            Verb::Record { seconds, .. } => Command::Record { seconds: *seconds },
            Verb::History { command: History::List { limit, search } } => {
                Command::HistoryList { limit: *limit as usize, search: search.clone() }
            }
            Verb::History { command: History::Retry { id } } => Command::HistoryRetry { id: *id },
            Verb::Transcribe { file, srt, timestamps, cleanup, .. } => {
                let path = std::fs::canonicalize(file)
                    .map_err(|_| Error::new("file_read", "Could not open the input file."))?;
                if !path.is_file() {
                    return Err(Error::new("file_read", "The input must be a file."));
                }
                let path =
                    path.to_str().ok_or_else(|| Error::new("file_read", "The file path must be valid Unicode."))?;
                Command::Transcribe {
                    path: path.into(),
                    format: if *srt { Format::Srt } else { Format::Txt },
                    timestamps: *timestamps,
                    cleanup: *cleanup,
                }
            }
            Verb::Profiles { command: Profiles::List } => Command::ProfilesList,
            Verb::Profile { command: Profile::Use { id } } => Command::ProfileUse { id: id.clone() },
            Verb::Version => Command::Version,
        };
        let request = Request::new(command);
        request.validate()?;
        Ok(request)
    }
}

#[cfg(windows)]
async fn call(request: &Request) -> Result<Response, Error> {
    opit_ipc::windows::call(request).await
}

#[cfg(unix)]
async fn call(request: &Request) -> Result<Response, Error> {
    opit_ipc::unix::call(request).await
}

fn exit_code(error: &Error) -> u8 {
    if error.code == "app_not_running" { 3 } else { 1 }
}

fn plain(value: &Value) -> String {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.into();
    }
    if let Some(version) = value.get("version").and_then(Value::as_str) {
        return version.into();
    }
    if let Some(rows) = value.as_array() {
        return rows
            .iter()
            .map(|row| {
                format!(
                    "{}\t{}",
                    row.get("id").unwrap_or(&Value::Null),
                    row.get("text").or_else(|| row.get("name")).and_then(Value::as_str).unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn report(response: Response, json_output: bool) -> ExitCode {
    let code = match &response.outcome {
        Outcome::Ok { .. } => 0,
        Outcome::Err { error } => exit_code(error),
    };
    if json_output {
        println!("{}", serde_json::to_string(&response).expect("serializable response"));
    } else {
        match &response.outcome {
            Outcome::Ok { result } => println!("{}", plain(result)),
            Outcome::Err { error } => eprintln!("{}: {}", error.code, error.message),
        }
    }
    ExitCode::from(code)
}

#[tokio::main]
async fn main() -> ExitCode {
    let raw: Vec<_> = std::env::args_os().collect();
    let json_output = raw.iter().any(|a| a == "--json");
    let args = match Args::try_parse_from(raw) {
        Ok(args) => args,
        Err(error) => {
            if matches!(error.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion) {
                print!("{error}");
                return ExitCode::SUCCESS;
            }
            let safe = Error::new("usage", "Invalid arguments. Run opit --help for usage.");
            report(Response::from_result(Err(safe)), json_output);
            return ExitCode::from(2);
        }
    };
    if matches!(args.verb, Verb::Version) {
        return report(Response::from_result(Ok(json!({"version":env!("CARGO_PKG_VERSION")}))), args.json);
    }
    let result = match args.request() {
        Ok(request) => call(&request).await,
        Err(error) => Err(error),
    };
    let mut response = result.unwrap_or_else(|e| Response::from_result(Err(e)));
    if let Verb::Transcribe { output: Some(path), .. } = &args.verb
        && let Outcome::Ok { result } = &response.outcome
    {
        let saved = result
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("protocol", "Missing transcription result."))
            .and_then(|text| {
                std::fs::write(path, text).map_err(|_| Error::new("file_write", "Could not write the output file."))
            });
        if let Err(error) = saved {
            response = Response::from_result(Err(error));
        }
    }
    report(response, args.json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_verbs_support_global_json() {
        for command in [
            "status",
            "toggle",
            "start",
            "stop",
            "ask",
            "cancel",
            "pause",
            "resume",
            "retry",
            "version",
            "record --seconds 2 --no-paste",
            "history list --limit 3 --search sample",
            "history retry 1",
            "profiles list",
            "profile use groq",
            "transcribe test.wav --srt --timestamps --cleanup -o out.srt",
        ] {
            let words: Vec<_> = std::iter::once("opit").chain(command.split_whitespace()).chain(["--json"]).collect();
            assert!(Args::try_parse_from(words).unwrap().json);
        }
    }

    #[test]
    fn ask_preserves_text_and_defaults_to_microphone_capture() {
        let args = Args::try_parse_from(["opit", "ask"]).unwrap();
        assert!(matches!(args.request().unwrap().command, Command::Ask { text: None }));
        let text = "request with quotes \" & --danger";
        let args = Args::try_parse_from(["opit", "ask", "--text", text]).unwrap();
        match args.request().unwrap().command {
            Command::Ask { text: Some(request) } => assert_eq!(request, text),
            _ => panic!("expected text request"),
        }
    }

    #[test]
    fn invalid_usage_and_safe_record_default() {
        for command in [
            "record",
            "record --seconds 0",
            "record --seconds 301",
            "history retry -1",
            "history list --limit 1001",
            "transcribe a.wav --srt --txt",
            "unknown",
        ] {
            assert!(Args::try_parse_from(std::iter::once("opit").chain(command.split_whitespace())).is_err());
        }
        let args = Args::try_parse_from(["opit", "record", "--seconds", "5"]).unwrap();
        assert!(matches!(args.request().unwrap().command, Command::Record { seconds: 5 }));
        assert_eq!(exit_code(&Error::new("app_not_running", "")), 3);
        assert_eq!(exit_code(&Error::new("provider", "")), 1);
    }
}
