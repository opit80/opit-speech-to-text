//! Deterministic accuracy layer: rule packs, matching, prompt building.

pub mod engine;
pub mod hallucination;
pub mod matcher;
pub mod pack;

pub use engine::{RuleHit, RuleKind, RuleRef, RuleSet, RuleWarning};
pub use pack::{PackError, Replacement, RulePack, load_pack_file};
