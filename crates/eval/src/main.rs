//! `opit-eval`: measure WER and term accuracy of a provider profile on a local dataset.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use opit_core::provider::retry::RETRY_DELAY;
use opit_core::provider::{Profile, presets};
use opit_core::rules::builtin::assemble;
use opit_core::rules::{RuleSet, load_pack_file};
use opit_eval::run::{EvalConfig, RulesMode, run_eval};
use opit_eval::{dataset, report};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Preset {
    Groq,
    Openai,
    Custom,
}

/// Measure WER and term accuracy of a provider profile on a folder of wav+txt pairs.
#[derive(Debug, Parser)]
#[command(name = "opit-eval", version)]
struct Args {
    /// Folder with `name.wav` + `name.txt` pairs.
    #[arg(long)]
    dir: PathBuf,
    #[arg(long, value_enum, default_value_t = Preset::Groq)]
    preset: Preset,
    /// Override the preset's base URL (required for `custom`).
    #[arg(long)]
    base_url: Option<String>,
    /// Override the preset's model (required for `custom`).
    #[arg(long)]
    model: Option<String>,
    /// Transcription language (ISO-639-1 or `auto`).
    #[arg(long, default_value = "tr")]
    language: String,
    #[arg(long, value_enum, default_value_t = RulesMode::Both)]
    rules: RulesMode,
    /// Built-in packs to enable, comma-separated.
    #[arg(long, value_delimiter = ',', default_value = "tr-core,tr-tech")]
    packs: Vec<String>,
    /// Personal rule pack (user.yaml), applied with the highest priority.
    #[arg(long)]
    user_rules: Option<PathBuf>,
    /// Prompt context sentence.
    #[arg(long, default_value = "")]
    context: String,
    /// Environment variable holding the API key (default: GROQ_API_KEY / OPENAI_API_KEY / OPIT_API_KEY).
    #[arg(long)]
    api_key_env: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let profile = build_profile(&args)?;
    let key_var = args.api_key_env.clone().unwrap_or_else(|| default_key_var(args.preset).to_string());
    let api_key = std::env::var(&key_var).ok().filter(|k| !k.trim().is_empty());
    if api_key.is_none() && !matches!(args.preset, Preset::Custom) {
        bail!("set {key_var} to your API key");
    }

    let user = args.user_rules.as_deref().map(load_pack_file).transpose().context("cannot load --user-rules")?;
    let (rules, warnings) = RuleSet::compile(&assemble(user, &args.packs));
    for warning in &warnings {
        eprintln!("warning [{}]: {}", warning.pack_id, warning.message);
    }

    let (samples, skipped) = dataset::load_dir(&args.dir)?;
    for message in &skipped {
        eprintln!("warning: {message}");
    }
    if samples.is_empty() {
        bail!("no wav+txt pairs found in {}", args.dir.display());
    }

    let config = EvalConfig {
        profile,
        api_key,
        rules,
        prompt_context: args.context.clone(),
        mode: args.rules,
        retry_delay: RETRY_DELAY,
    };
    let results = run_eval(&samples, &config).await?;
    print!("{}", report::render(&results));
    Ok(())
}

fn build_profile(args: &Args) -> Result<Profile> {
    let mut profile = match args.preset {
        Preset::Groq => presets::groq(),
        Preset::Openai => presets::openai(),
        Preset::Custom => {
            let (Some(url), Some(model)) = (&args.base_url, &args.model) else {
                bail!("--preset custom needs --base-url and --model");
            };
            presets::custom("custom", "Custom", url, model)
        }
    };
    if let Some(url) = &args.base_url {
        profile.base_url = url.clone();
    }
    if let Some(model) = &args.model {
        profile.model = model.clone();
    }
    profile.language = args.language.clone();
    Ok(profile)
}

fn default_key_var(preset: Preset) -> &'static str {
    match preset {
        Preset::Groq => "GROQ_API_KEY",
        Preset::Openai => "OPENAI_API_KEY",
        Preset::Custom => "OPIT_API_KEY",
    }
}
