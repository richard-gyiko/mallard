use std::path::Path;

use serde::Deserialize;

use crate::core::{FileId, Finding, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct RuleDef {
    pub id: String,
    pub pattern: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RuleFile {
    rules: Vec<RuleDef>,
}

#[derive(Debug, Clone)]
pub struct RuleSet {
    pub rules: Vec<RuleDef>,
    pub source_hash: Option<String>,
}

impl RuleSet {
    pub fn empty() -> Self {
        RuleSet {
            rules: Vec::new(),
            source_hash: None,
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        let file: RuleFile = serde_yaml::from_slice(&bytes)?;
        let hash = blake3::hash(&bytes).to_hex().as_str()[..32].to_string();
        Ok(RuleSet {
            rules: file.rules,
            source_hash: Some(hash),
        })
    }

    pub fn run(&self, _file_id: FileId, _source: &[u8], _language: &str) -> Vec<Finding> {
        // v0: rule execution deferred. The findings table exists and is queryable;
        // a future version will run patterns via ast-grep-core. Rule set metadata
        // (id, hash) is still tracked so re-builds with the same rules stay
        // deterministic.
        Vec::new()
    }
}
