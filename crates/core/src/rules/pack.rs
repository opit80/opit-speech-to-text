//! Rule pack YAML model (`schema: 1`).

use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::text::fold;

pub const PACK_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulePack {
    pub schema: u32,
    pub id: String,
    pub name: String,
    #[serde(default = "default_language")]
    pub language: String,
    /// Canonical spellings: fed to the prompt and enforced by casing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terms: Vec<String>,
    /// Canonical form → misrecognized variants.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub corrections: IndexMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replacements: Vec<Replacement>,
    /// Whole-output patterns that mean the model hallucinated on silence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hallucinations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub case_sensitive: bool,
    /// `from` is a regex used as-is (no implicit word boundaries); `to` may use `$1`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub regex: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    #[error("invalid rule pack YAML: {0}")]
    Yaml(String),
    #[error("unsupported rule pack schema {found} (expected {PACK_SCHEMA})")]
    Schema { found: u32 },
    #[error("rule pack id must be lowercase letters, digits or '-': {0:?}")]
    BadId(String),
    #[error("cannot read rule pack: {0}")]
    Io(String),
}

impl RulePack {
    pub fn from_yaml(src: &str) -> Result<Self, PackError> {
        let pack: RulePack = serde_saphyr::from_str(src).map_err(|e| PackError::Yaml(e.to_string()))?;
        if pack.schema != PACK_SCHEMA {
            return Err(PackError::Schema { found: pack.schema });
        }
        let id_ok =
            !pack.id.is_empty() && pack.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err(PackError::BadId(pack.id));
        }
        Ok(pack)
    }

    pub fn to_yaml(&self) -> Result<String, PackError> {
        serde_saphyr::to_string(self).map_err(|e| PackError::Yaml(e.to_string()))
    }

    pub fn is_turkish(&self) -> bool {
        self.language == "tr"
    }

    /// YAML for this pack with `header` (see [`header_comments`]) on top.
    pub fn to_yaml_with_header(&self, header: &str) -> Result<String, PackError> {
        let body = self.to_yaml()?;
        if header.is_empty() { Ok(body) } else { Ok(format!("{header}\n{body}")) }
    }

    /// Records that `variant` is a misrecognition of `canonical`. Spellings that differ only
    /// in case (Turkish-aware) add `canonical` to `terms` instead, since casing is fixed there.
    pub fn add_correction(&mut self, canonical: &str, variant: &str) -> Result<CorrectionOutcome, CorrectionError> {
        let (canonical, variant) = (collapse_ws(canonical), collapse_ws(variant));
        if canonical.is_empty() || variant.is_empty() {
            return Err(CorrectionError::Empty);
        }
        if canonical == variant {
            return Err(CorrectionError::SameAsCanonical);
        }
        let folded = fold(&variant);
        if fold(&canonical) == folded {
            if self.terms.contains(&canonical) {
                return Ok(CorrectionOutcome::AlreadyPresent);
            }
            self.terms.push(canonical);
            return Ok(CorrectionOutcome::AddedTerm);
        }
        for (key, variants) in &self.corrections {
            if *key != canonical && variants.iter().any(|v| fold(v) == folded) {
                return Err(CorrectionError::TakenBy { variant: variant.clone(), canonical: key.clone() });
            }
        }
        let variants = self.corrections.entry(canonical).or_default();
        if variants.iter().any(|v| fold(v) == folded) {
            return Ok(CorrectionOutcome::AlreadyPresent);
        }
        variants.push(variant);
        Ok(CorrectionOutcome::AddedVariant)
    }
}

/// The comment block at the top of a pack file: the leading lines that are blank or start
/// with `#`, without trailing blank lines. Returned with a final newline, or empty.
pub fn header_comments(yaml: &str) -> String {
    let yaml = yaml.strip_prefix('\u{FEFF}').unwrap_or(yaml);
    let mut header = String::new();
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        if !(trimmed.is_empty() || trimmed.starts_with('#')) {
            break;
        }
        header.push_str(line.trim_end());
        header.push('\n');
    }
    let kept = header.trim_end().len();
    if kept == 0 {
        return String::new();
    }
    header.truncate(kept);
    header.push('\n');
    header
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionOutcome {
    AddedVariant,
    /// The two spellings differ only in case, so the canonical form became a term.
    AddedTerm,
    AlreadyPresent,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CorrectionError {
    #[error("the correct and the wrong spelling must not be empty")]
    Empty,
    #[error("the wrong spelling is the same as the correct one")]
    SameAsCanonical,
    #[error("{variant:?} is already corrected to {canonical:?}")]
    TakenBy { variant: String, canonical: String },
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn load_pack_file(path: &Path) -> Result<RulePack, PackError> {
    let src = std::fs::read_to_string(path).map_err(|e| PackError::Io(format!("{}: {e}", path.display())))?;
    RulePack::from_yaml(&src)
}

fn default_language() -> String {
    "tr".to_string()
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
schema: 1
id: tr-tech
name: Turkish – software terms
language: tr
terms: [Claude Code, GitHub]
corrections:
  Claude Code: [cloud code, clod code]
replacements:
  - { from: hala, to: hâlâ }
  - { from: '(\d+) tl', to: '$1 TL', regex: true, case_sensitive: true }
hallucinations: [altyazı m.k]
"#;

    #[test]
    fn parses_a_full_pack() {
        let pack = RulePack::from_yaml(FULL).unwrap();
        assert_eq!(pack.id, "tr-tech");
        assert_eq!(pack.terms, vec!["Claude Code", "GitHub"]);
        assert_eq!(pack.corrections["Claude Code"], vec!["cloud code", "clod code"]);
        assert_eq!(pack.replacements[1].to, "$1 TL");
        assert!(pack.replacements[1].regex);
        assert!(pack.replacements[1].case_sensitive);
        assert!(!pack.replacements[0].regex);
        assert!(pack.is_turkish());
    }

    #[test]
    fn optional_sections_default_to_empty() {
        let pack = RulePack::from_yaml("schema: 1\nid: min\nname: Minimal\n").unwrap();
        assert_eq!(pack.language, "tr");
        assert!(pack.terms.is_empty() && pack.corrections.is_empty());
        assert!(pack.replacements.is_empty() && pack.hallucinations.is_empty());
    }

    #[test]
    fn rejects_other_schema_versions() {
        let err = RulePack::from_yaml("schema: 2\nid: x\nname: X\n").unwrap_err();
        assert!(matches!(err, PackError::Schema { found: 2 }));
    }

    #[test]
    fn rejects_unknown_fields_with_their_name() {
        let err = RulePack::from_yaml("schema: 1\nid: x\nname: X\ncorections: {}\n").unwrap_err();
        assert!(matches!(&err, PackError::Yaml(msg) if msg.contains("corections")), "{err}");
    }

    #[test]
    fn rejects_bad_ids() {
        for id in ["", "Tr Core", "tr_core", "ç"] {
            let src = format!("schema: 1\nid: '{id}'\nname: X\n");
            assert!(matches!(RulePack::from_yaml(&src), Err(PackError::BadId(_))), "{id:?}");
        }
    }

    #[test]
    fn yaml_round_trip() {
        let pack = RulePack::from_yaml(FULL).unwrap();
        let again = RulePack::from_yaml(&pack.to_yaml().unwrap()).unwrap();
        assert_eq!(pack, again);
    }

    #[test]
    fn loads_from_file_and_reports_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("user.yaml");
        std::fs::write(&path, FULL).unwrap();
        assert_eq!(load_pack_file(&path).unwrap().id, "tr-tech");
        let missing = load_pack_file(&dir.path().join("nope.yaml")).unwrap_err();
        assert!(matches!(missing, PackError::Io(_)));
    }

    const WITH_HEADER: &str = "# Personal rules\n# prompt_context: FiveM talk\n\nschema: 1\nid: user\nname: Me\n# inline note\nterms: [Opit]\n";

    #[test]
    fn header_comments_takes_only_the_leading_block() {
        assert_eq!(header_comments(WITH_HEADER), "# Personal rules\n# prompt_context: FiveM talk\n");
        assert_eq!(header_comments("schema: 1\n# late\n"), "");
        assert_eq!(header_comments("\u{FEFF}# bom\nschema: 1\n"), "# bom\n");
        assert_eq!(header_comments(""), "");
    }

    #[test]
    fn to_yaml_with_header_keeps_the_header_and_is_stable() {
        let pack = RulePack::from_yaml(WITH_HEADER).unwrap();
        let header = header_comments(WITH_HEADER);
        let once = pack.to_yaml_with_header(&header).unwrap();
        assert!(once.starts_with("# Personal rules\n# prompt_context: FiveM talk\n"));
        assert_eq!(RulePack::from_yaml(&once).unwrap(), pack);
        let twice = RulePack::from_yaml(&once).unwrap().to_yaml_with_header(&header_comments(&once)).unwrap();
        assert_eq!(once, twice);
    }

    fn user_pack() -> RulePack {
        RulePack::from_yaml("schema: 1\nid: user\nname: Me\ncorrections:\n  Claude Code: [cloud code]\n").unwrap()
    }

    #[test]
    fn add_correction_appends_a_variant_once() {
        let mut pack = user_pack();
        assert_eq!(pack.add_correction("Claude Code", "klod kod"), Ok(CorrectionOutcome::AddedVariant));
        assert_eq!(pack.add_correction(" Claude  Code ", "KLOD KOD"), Ok(CorrectionOutcome::AlreadyPresent));
        assert_eq!(pack.corrections["Claude Code"], ["cloud code", "klod kod"]);
    }

    #[test]
    fn add_correction_creates_a_new_canonical_entry() {
        let mut pack = user_pack();
        assert_eq!(pack.add_correction("Opit", "opet"), Ok(CorrectionOutcome::AddedVariant));
        assert_eq!(pack.corrections["Opit"], ["opet"]);
    }

    #[test]
    fn a_case_only_difference_becomes_a_term() {
        let mut pack = user_pack();
        assert_eq!(pack.add_correction("GitHub", "github"), Ok(CorrectionOutcome::AddedTerm));
        assert_eq!(pack.add_correction("GitHub", "Github"), Ok(CorrectionOutcome::AlreadyPresent));
        assert_eq!(pack.terms, ["GitHub"]);
        // Turkish dotted/dotless I fold together, so this is also case-only.
        assert_eq!(pack.add_correction("İstanbul", "istanbul"), Ok(CorrectionOutcome::AddedTerm));
    }

    #[test]
    fn add_correction_rejects_bad_input() {
        let mut pack = user_pack();
        assert_eq!(pack.add_correction("", "x"), Err(CorrectionError::Empty));
        assert_eq!(pack.add_correction("x", "  "), Err(CorrectionError::Empty));
        assert_eq!(pack.add_correction("Opit", "Opit"), Err(CorrectionError::SameAsCanonical));
        assert_eq!(
            pack.add_correction("Kod", "Claude Code"),
            Ok(CorrectionOutcome::AddedVariant),
            "a variant equal to another canonical (not to one of its variants) is allowed"
        );
        assert_eq!(
            pack.add_correction("Clod", "cloud code"),
            Err(CorrectionError::TakenBy { variant: "cloud code".into(), canonical: "Claude Code".into() })
        );
    }
}
