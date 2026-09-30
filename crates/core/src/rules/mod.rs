//! Deterministic accuracy layer: rule packs, matching, prompt building.

pub mod builtin;
pub mod engine;
pub mod hallucination;
pub mod matcher;
pub mod pack;
pub mod prompt;

pub use engine::{RuleHit, RuleKind, RuleRef, RuleSet, RuleWarning};
pub use pack::{PackError, Replacement, RulePack, load_pack_file};
