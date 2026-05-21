use std::str::FromStr;

use ast_grep_language::SupportLang;

use crate::core::{
    FileId, FileRecord, FileStatus, FileTiming, ParsedFile, ProcessOutcome, Result,
};
use crate::parsed_source::ParsedSource;
use crate::parser::RustParser;
use crate::rules::RuleSet;
use crate::walk::WalkEntry;

pub struct FileProcessor {
    parser: RustParser,
    rules: RuleSet,
}

impl FileProcessor {
    pub fn new(rules: RuleSet) -> Result<Self> {
        Ok(FileProcessor {
            parser: RustParser::new()?,
            rules,
        })
    }

    pub fn rule_set_hash(&self) -> Option<&str> {
        self.rules.source_hash.as_deref()
    }

    pub fn process(&mut self, file_id: FileId, entry: &WalkEntry) -> Result<ProcessOutcome> {
        let file_record = FileRecord {
            id: file_id,
            path: entry.relative_path.clone(),
            language: entry.language.clone(),
            size_bytes: entry.size_bytes,
            status: entry.status,
        };

        if entry.status != FileStatus::Indexed {
            return Ok(ProcessOutcome {
                file_record,
                parsed: None,
                findings: Vec::new(),
                timing: None,
            });
        }

        let raw = std::fs::read(&entry.path)?;
        let Ok(source) = std::str::from_utf8(&raw) else {
            return Ok(ProcessOutcome {
                file_record: FileRecord {
                    status: FileStatus::SkippedBinary,
                    ..file_record
                },
                parsed: None,
                findings: Vec::new(),
                timing: None,
            });
        };
        let lang = entry
            .language
            .as_deref()
            .and_then(|l| SupportLang::from_str(l).ok());
        let Some(lang) = lang else {
            return Ok(ProcessOutcome {
                file_record: FileRecord {
                    status: FileStatus::SkippedExtension,
                    ..file_record
                },
                parsed: None,
                findings: Vec::new(),
                timing: None,
            });
        };

        let parsed_source = ParsedSource::parse(lang, source)?;

        let t_rules = std::time::Instant::now();
        let findings = self.rules.run(file_id, &parsed_source);
        let rules_ms = t_rules.elapsed().as_millis() as u64;

        let parsed: ParsedFile =
            self.parser
                .extract_from(&parsed_source, file_id, &entry.relative_path);

        let timing = FileTiming {
            path: entry.relative_path.clone(),
            parse_ms: parsed.parse_ms,
            query_ms: parsed.query_ms,
            rules_ms,
        };

        Ok(ProcessOutcome {
            file_record,
            parsed: Some(parsed),
            findings,
            timing: Some(timing),
        })
    }
}
