use std::path::Path;

use serde::Deserialize;

use crate::core::{FileId, Finding, Result, short_hash};

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
        Ok(RuleSet {
            rules: file.rules,
            source_hash: Some(short_hash(blake3::hash(&bytes))),
        })
    }

    pub fn run(&self, _file_id: FileId, _source: &[u8], _language: &str) -> Vec<Finding> {
        Vec::new()
    }
}
