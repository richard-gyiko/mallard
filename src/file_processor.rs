use crate::core::{
    FileId, FileRecord, FileStatus, FileTiming, ProcessOutcome, Result,
};
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

        let source = std::fs::read(&entry.path)?;
        let language = entry.language.as_deref().unwrap_or_default();

        let t_rules = std::time::Instant::now();
        let findings = self.rules.run(file_id, &source, language);
        let rules_ms = t_rules.elapsed().as_millis() as u64;

        let parsed = self.parser.parse_file(file_id, &entry.relative_path, &source)?;

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
