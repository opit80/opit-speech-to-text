//! Rule pack YAML model (`schema: 1`).

use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

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
}
