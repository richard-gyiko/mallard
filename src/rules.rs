use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use ast_grep_core::{Pattern, matcher::PatternError};
use ast_grep_language::{LanguageExt, SupportLang};
use serde::Deserialize;

use crate::core::{FileId, Finding, MallardError, Result, short_hash};

#[derive(Debug, Clone, Deserialize)]
pub struct RuleDef {
    pub id: String,
    pub language: String,
    pub pattern: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RuleFile {
    rules: Vec<RuleDef>,
}

struct CompiledRule {
    id: String,
    message: String,
    pattern: Pattern,
}

pub struct RuleSet {
    by_language: HashMap<SupportLang, Vec<CompiledRule>>,
    pub source_hash: Option<String>,
}

impl RuleSet {
    pub fn empty() -> Self {
        RuleSet {
            by_language: HashMap::new(),
            source_hash: None,
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        let file: RuleFile = serde_yaml::from_slice(&bytes)?;
        let mut by_language: HashMap<SupportLang, Vec<CompiledRule>> = HashMap::new();
        for rule in file.rules {
            let lang = SupportLang::from_str(&rule.language).map_err(|_| {
                MallardError::Other(format!(
                    "rule {:?}: unknown language {:?}",
                    rule.id, rule.language
                ))
            })?;
            let pattern = Pattern::try_new(&rule.pattern, lang).map_err(|e: PatternError| {
                MallardError::Other(format!("rule {:?}: invalid pattern: {e}", rule.id))
            })?;
            by_language.entry(lang).or_default().push(CompiledRule {
                id: rule.id,
                message: rule.message,
                pattern,
            });
        }
        Ok(RuleSet {
            by_language,
            source_hash: Some(short_hash(blake3::hash(&bytes))),
        })
    }

    pub fn run(&self, file_id: FileId, source: &[u8], language: &str) -> Vec<Finding> {
        let Ok(lang) = SupportLang::from_str(language) else {
            return Vec::new();
        };
        let Some(rules) = self.by_language.get(&lang) else {
            return Vec::new();
        };
        let Ok(src) = std::str::from_utf8(source) else {
            return Vec::new();
        };

        let ast = lang.ast_grep(src);
        let root = ast.root();
        let mut findings: Vec<Finding> = Vec::new();
        for rule in rules {
            for m in root.find_all(&rule.pattern) {
                findings.push(Finding {
                    rule_id: rule.id.clone(),
                    file_id,
                    start_line: m.start_pos().line() as u32,
                    end_line: m.end_pos().line() as u32,
                    message: rule.message.clone(),
                });
            }
        }
        findings.sort_by(|a, b| {
            a.rule_id
                .cmp(&b.rule_id)
                .then(a.start_line.cmp(&b.start_line))
                .then(a.end_line.cmp(&b.end_line))
        });
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_rules(yaml: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();
        f
    }

    #[test]
    fn loads_pattern_rule() {
        let f = write_rules(
            r#"
rules:
  - id: rust-unwrap
    language: rust
    pattern: "$X.unwrap()"
    message: "no unwrap"
"#,
        );
        let rs = RuleSet::load(f.path()).unwrap();
        assert!(rs.source_hash.is_some());
        assert_eq!(rs.by_language.get(&SupportLang::Rust).unwrap().len(), 1);
    }

    #[test]
    fn rejects_unknown_language() {
        let f = write_rules(
            r#"
rules:
  - id: x
    language: cobol
    pattern: "x"
"#,
        );
        let err = RuleSet::load(f.path()).err().expect("should fail");
        assert!(
            err.to_string().to_lowercase().contains("cobol"),
            "got: {err}"
        );
    }

    #[test]
    fn matches_unwrap_pattern_rust() {
        let f = write_rules(
            r#"
rules:
  - id: rust-unwrap
    language: rust
    pattern: "$X.unwrap()"
    message: "no unwrap"
"#,
        );
        let rs = RuleSet::load(f.path()).unwrap();
        let src = b"fn f() {\n    let v = thing.unwrap();\n}\n";
        let findings = rs.run(7, src, "rust");
        assert_eq!(findings.len(), 1);
        let f0 = &findings[0];
        assert_eq!(f0.rule_id, "rust-unwrap");
        assert_eq!(f0.file_id, 7);
        assert_eq!(f0.message, "no unwrap");
        assert_eq!(f0.start_line, 1);
        assert_eq!(f0.end_line, 1);
    }

    #[test]
    fn deterministic_ordering() {
        let f = write_rules(
            r#"
rules:
  - id: rule-b
    language: rust
    pattern: "$X.unwrap()"
  - id: rule-a
    language: rust
    pattern: "$X.expect($M)"
"#,
        );
        let rs = RuleSet::load(f.path()).unwrap();
        let src = b"fn f() {\n    a.unwrap();\n    b.expect(\"x\");\n    c.unwrap();\n}\n";
        let findings = rs.run(1, src, "rust");
        let ids: Vec<&str> = findings.iter().map(|f| f.rule_id.as_str()).collect();
        let lines: Vec<u32> = findings.iter().map(|f| f.start_line).collect();
        assert_eq!(ids, vec!["rule-a", "rule-b", "rule-b"]);
        assert_eq!(lines, vec![2, 1, 3]);
    }

    #[test]
    fn unknown_language_at_runtime_returns_empty() {
        let rs = RuleSet::empty();
        assert!(rs.run(1, b"x", "cobol").is_empty());
    }
}
