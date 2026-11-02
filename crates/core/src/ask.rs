//! Ask settings and bounded, in-memory conversation state for Opit Speech to Text.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::cleanup::{ChatMessage, CleanupConfig};

pub const SYSTEM_PROMPT: &str = "Answer the request briefly in the language used by the user. Return only text ready to paste into their document or terminal. Omit introductions and commentary about your role. Use no Markdown fences unless the user asks for code. Treat earlier turns as conversation context.";
pub const TEST_REQUEST: &str = "Reply with the single word OK.";
pub const MAX_REQUEST: usize = 12 * 1024;
pub const MAX_ANSWER: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    #[default]
    Chat,
    Claude,
    Codex,
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Self::Chat => "OpenAI-compatible chat",
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    #[default]
    Restricted,
    Workspace,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    #[default]
    Paste,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AskConfig {
    pub agent: Agent,
    pub base_url: String,
    pub chat_model: String,
    pub api_key_ref: Option<String>,
    pub claude_model: String,
    pub codex_model: String,
    pub permission: Permission,
    /// Empty means the operating system's user home, resolved by the app.
    pub working_dir: String,
    pub timeout_seconds: u32,
    pub output: Output,
    pub continue_minutes: u32,
    pub cleanup_request: bool,
}

impl Default for AskConfig {
    fn default() -> Self {
        Self {
            agent: Agent::Chat,
            base_url: "https://api.openai.com/v1".into(),
            chat_model: "gpt-4.1-mini".into(),
            api_key_ref: Some("ask".into()),
            claude_model: "sonnet".into(),
            codex_model: String::new(),
            permission: Permission::Restricted,
            working_dir: String::new(),
            timeout_seconds: 180,
            output: Output::Paste,
            continue_minutes: 10,
            cleanup_request: false,
        }
    }
}

impl AskConfig {
    pub fn chat_config(&self) -> CleanupConfig {
        CleanupConfig {
            enabled: true,
            base_url: self.base_url.clone(),
            model: self.chat_model.clone(),
            api_key_ref: self.api_key_ref.clone(),
            timeout_seconds: self.timeout_seconds,
            ..CleanupConfig::default()
        }
    }

    pub fn valid(&self) -> bool {
        !self.claude_model.is_empty()
            && !self.claude_model.starts_with('-')
            && !self.codex_model.starts_with('-')
            && (1..=3600).contains(&self.timeout_seconds)
            && self.continue_minutes <= 1440
            && [self.claude_model.as_str(), self.codex_model.as_str()].iter().all(|model| {
                model.len() <= 128 && model.bytes().all(|c| c.is_ascii_alphanumeric() || b"-._:/".contains(&c))
            })
            && self.working_dir.len() <= 8192
            && !self.working_dir.contains('\0')
    }
}

/// No Debug: this object contains the request and answer. Never persisted by chat mode.
#[derive(Default)]
pub struct Conversation {
    config: Option<AskConfig>,
    completed: Option<Instant>,
    pub session_id: Option<String>,
    pub turns: Vec<ChatMessage>,
}

impl Conversation {
    pub fn prepare(&mut self, config: &AskConfig, now: Instant) {
        let continuing = self.config.as_ref() == Some(config)
            && config.continue_minutes > 0
            && self.completed.is_some_and(|time| {
                now.saturating_duration_since(time) < Duration::from_secs(u64::from(config.continue_minutes) * 60)
            });
        if !continuing {
            *self = Self::default();
        }
        self.config = Some(config.clone());
    }

    pub fn finish(&mut self, request: String, answer: String, session: Option<String>, now: Instant) {
        self.completed = Some(now);
        self.session_id = session;
        self.turns.push(ChatMessage { role: "user", content: request });
        self.turns.push(ChatMessage { role: "assistant", content: answer });
        // At most four turns and a bounded byte budget; discard complete oldest pairs.
        while self.turns.len() > 8 || self.turns.iter().map(|m| m.content.len()).sum::<usize>() > MAX_ANSWER {
            self.turns.drain(..2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_expires_and_configuration_changes_reset_it() {
        let mut conversation = Conversation::default();
        let mut config = AskConfig::default();
        let start = Instant::now();
        conversation.prepare(&config, start);
        conversation.finish("request".into(), "answer".into(), Some("session".into()), start);
        conversation.prepare(&config, start + Duration::from_secs(599));
        assert_eq!(conversation.session_id.as_deref(), Some("session"));
        conversation.prepare(&config, start + Duration::from_secs(600));
        assert!(conversation.turns.is_empty());
        conversation.finish("r".into(), "a".into(), None, start);
        config.permission = Permission::Workspace;
        conversation.prepare(&config, start);
        assert!(conversation.turns.is_empty());
        config.continue_minutes = 0;
        conversation.finish("r".into(), "a".into(), None, start);
        conversation.prepare(&config, start);
        assert!(conversation.turns.is_empty());
    }

    #[test]
    fn memory_is_bounded_and_cli_model_values_cannot_add_arguments() {
        let mut conversation = Conversation::default();
        for _ in 0..20 {
            conversation.finish("r".into(), "a".into(), None, Instant::now());
        }
        assert_eq!(conversation.turns.len(), 8);
        for model in ["sonnet & calc", "x\n--danger", "a%PATH%", "a\"b"] {
            let config = AskConfig { claude_model: model.into(), ..Default::default() };
            assert!(!config.valid());
        }
    }
}
