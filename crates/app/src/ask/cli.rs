//! Fixed CLI argument vectors and JSON decoders. The request is never an argument.
use crate::controller::ErrorKind;
use opit_core::ask::{Agent, AskConfig, MAX_ANSWER, Permission};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct AgentStatus {
    agent: Agent,
    installed: bool,
}

pub fn statuses() -> Vec<AgentStatus> {
    [Agent::Claude, Agent::Codex]
        .into_iter()
        .map(|agent| AgentStatus { agent, installed: resolve(agent).is_some() })
        .collect()
}

pub fn resolve(agent: Agent) -> Option<PathBuf> {
    resolve_in(
        agent,
        &std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default(),
    )
}

fn resolve_in(agent: Agent, directories: &[PathBuf]) -> Option<PathBuf> {
    let name = match agent {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
        Agent::Chat => return None,
    };
    for directory in directories.iter().filter(|d| d.is_absolute()) {
        for extension in if cfg!(windows) { &[".exe", ".cmd", ".bat"][..] } else { &[""][..] } {
            let path = directory.join(format!("{name}{extension}"));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn session_valid(session: &str) -> bool {
    // Both CLIs emit UUID session ids. Never accept a flag, path, shell expansion or name.
    session.len() == 36
        && session.bytes().enumerate().all(
            |(i, c)| {
                if [8, 13, 18, 23].contains(&i) { c == b'-' } else { c.is_ascii_hexdigit() }
            },
        )
}

pub fn arguments(config: &AskConfig, session: Option<&str>) -> Result<Vec<String>, ErrorKind> {
    if !config.valid() || session.is_some_and(|s| !session_valid(s)) {
        return Err(ErrorKind::Rejected);
    }
    let mut args: Vec<String> = match config.agent {
        Agent::Chat => return Err(ErrorKind::Rejected),
        Agent::Claude => [
            "-p",
            "--output-format",
            "json",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            "{}",
            "--disable-slash-commands",
            "--settings",
            "{\"disableAllHooks\":true}",
            "--permission-mode",
            if config.permission == Permission::Restricted { "plan" } else { "acceptEdits" },
            "--tools",
            if config.permission == Permission::Restricted { "" } else { "Read,Glob,Grep,Edit,Write" },
            "--disallowedTools",
            "Bash,PowerShell,Task,Agent",
            "--model",
            &config.claude_model,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        Agent::Codex => {
            let mut args = vec!["exec".into()];
            if session.is_some() {
                args.push("resume".into());
            }
            args.extend(
                [
                    "--json",
                    "--skip-git-repo-check",
                    "--ignore-user-config",
                    "--ignore-rules",
                    "-c",
                    "approval_policy=\"never\"",
                    "-c",
                    if config.permission == Permission::Restricted {
                        "sandbox_mode=\"read-only\""
                    } else {
                        "sandbox_mode=\"workspace-write\""
                    },
                    "-c",
                    "web_search=\"disabled\"",
                    "-c",
                    "features.hooks=false",
                    "-c",
                    "features.shell_snapshot=false",
                ]
                .into_iter()
                .map(str::to_owned),
            );
            if config.permission == Permission::Restricted {
                args.extend(
                    [
                        "-c",
                        "features.shell_tool=false",
                        "-c",
                        "features.unified_exec=false",
                        "-c",
                        "features.js_repl=false",
                    ]
                    .into_iter()
                    .map(str::to_owned),
                );
            }
            if !config.codex_model.is_empty() {
                args.extend(["--model".into(), config.codex_model.clone()]);
            }
            args
        }
    };
    if let Some(session) = session {
        if config.agent == Agent::Claude {
            args.push("--resume".into());
        }
        args.push(session.into());
    }
    if config.agent == Agent::Codex {
        args.push("-".into());
    }
    Ok(args)
}

pub fn parse(agent: Agent, bytes: &[u8]) -> Result<(String, Option<String>), ErrorKind> {
    if bytes.len() > MAX_ANSWER {
        return Err(ErrorKind::TooLarge);
    }
    let (text, session) = match agent {
        Agent::Claude => {
            let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorKind::BadResponse)?;
            if value.get("is_error").and_then(Value::as_bool).unwrap_or(false)
                || value.get("subtype").and_then(Value::as_str).is_some_and(|s| s != "success")
            {
                return Err(ErrorKind::Rejected);
            }
            (
                value.get("result").and_then(Value::as_str).map(str::to_owned),
                value.get("session_id").and_then(Value::as_str).map(str::to_owned),
            )
        }
        Agent::Codex => {
            let mut text = None;
            let mut session = None;
            let mut completed = false;
            for line in bytes.split(|b| *b == b'\n').filter(|line| !line.iter().all(u8::is_ascii_whitespace)) {
                let value: Value = serde_json::from_slice(line).map_err(|_| ErrorKind::BadResponse)?;
                match value.get("type").and_then(Value::as_str) {
                    Some("thread.started") => {
                        session = value.get("thread_id").and_then(Value::as_str).map(str::to_owned)
                    }
                    Some("item.completed") if value["item"]["type"] == "agent_message" => {
                        text = value["item"]["text"].as_str().map(str::to_owned)
                    }
                    Some("turn.completed") => completed = true,
                    Some("error" | "turn.failed") => return Err(ErrorKind::Rejected),
                    _ => {}
                }
            }
            if !completed {
                return Err(ErrorKind::BadResponse);
            }
            (text, session)
        }
        Agent::Chat => return Err(ErrorKind::Rejected),
    };
    let text = text.filter(|s| !s.trim().is_empty()).ok_or(ErrorKind::BadResponse)?;
    if session.as_deref().is_some_and(|s| !session_valid(s)) {
        return Err(ErrorKind::BadResponse);
    }
    Ok((text.trim().into(), session))
}

pub(crate) fn is_shim(path: &Path) -> bool {
    path.extension().is_some_and(|s| s.eq_ignore_ascii_case("cmd") || s.eq_ignore_ascii_case("bat"))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "12345678-1234-1234-1234-123456789abc";
    #[test]
    fn argv_contains_only_settings_and_explicit_session_never_request() {
        for agent in [Agent::Claude, Agent::Codex] {
            let config = AskConfig { agent, ..Default::default() };
            let args = arguments(&config, Some(ID)).unwrap();
            assert!(args.iter().any(|s| s == ID));
            assert!(!args.iter().any(|s| s.contains("private request & --danger")));
            assert!(arguments(&config, Some("--last")).is_err());
            assert!(!args.iter().any(|s| s.contains("bypass")));
        }
        let args = arguments(&AskConfig { agent: Agent::Codex, ..Default::default() }, None).unwrap();
        assert!(args.contains(&"features.shell_tool=false".into()));
        assert!(args.contains(&"sandbox_mode=\"read-only\"".into()));
    }
    #[test]
    fn detection_uses_path_entries_and_native_or_cmd_shims() {
        let root = tempfile::tempdir().unwrap();
        assert!(resolve_in(Agent::Codex, &[root.path().into()]).is_none());
        let name = if cfg!(windows) { "codex.cmd" } else { "codex" };
        let path = root.path().join(name);
        std::fs::write(&path, "fake").unwrap();
        assert_eq!(resolve_in(Agent::Codex, &[root.path().into()]), Some(path));
        if cfg!(windows) {
            let exe = root.path().join("codex.exe");
            std::fs::write(&exe, "fake").unwrap();
            assert_eq!(resolve_in(Agent::Codex, &[root.path().into()]), Some(exe));
        }
    }
    #[test]
    fn parse_cli_results_and_reject_partial_or_error_outputs() {
        let claude = format!(r#"{{"result":"answer","session_id":"{ID}","subtype":"success","is_error":false}}"#);
        assert_eq!(parse(Agent::Claude, claude.as_bytes()).unwrap(), ("answer".into(), Some(ID.into())));
        let codex = format!(
            "{{\"type\":\"thread.started\",\"thread_id\":\"{ID}\"}}\n{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"answer\"}}}}\n{{\"type\":\"turn.completed\"}}\n"
        );
        assert_eq!(parse(Agent::Codex, codex.as_bytes()).unwrap().0, "answer");
        for input in [r#"{"is_error":true,"result":"secret error"}"#, "invalid secret", r#"{"result":""}"#] {
            assert!(parse(Agent::Claude, input.as_bytes()).is_err());
        }
        assert!(parse(Agent::Codex, b"{\"type\":\"turn.failed\"}").is_err());
        assert!(
            parse(
                Agent::Codex,
                b"{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"partial\"}}"
            )
            .is_err()
        );
    }
}
