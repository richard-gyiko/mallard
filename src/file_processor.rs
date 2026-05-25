use std::str::FromStr;

use ast_grep_language::SupportLang;

use crate::core::{
    FileId, FileRecord, FileStatus, FileTiming, ParsedFile, ProcessOutcome, Result,
};
use crate::extractor::{RustExtractor, SymbolExtractor};
use crate::extractor_python::PythonExtractor;
use crate::parsed_source::ParsedSource;
use crate::rules::RuleSet;
use crate::walk::WalkEntry;

pub struct FileProcessor {
    rust: RustExtractor,
    python: PythonExtractor,
    rules: RuleSet,
}

impl FileProcessor {
    pub fn new(rules: RuleSet) -> Result<Self> {
        Ok(FileProcessor {
            rust: RustExtractor::new()?,
            python: PythonExtractor::new()?,
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
            return Ok(skipped(file_record, FileStatus::SkippedBinary));
        };
        let lang = entry
            .language
            .as_deref()
            .and_then(|l| SupportLang::from_str(l).ok());
        let Some(lang) = lang else {
            return Ok(skipped(file_record, FileStatus::SkippedExtension));
        };
        // Per-language `SymbolExtractor` dispatch. Add new languages here as
        // additional arms + new fields on `FileProcessor`. See ADR-0012 for
        // the multi-language extractor architecture.
        let extractor: &mut dyn SymbolExtractor = match lang {
            SupportLang::Rust => &mut self.rust,
            SupportLang::Python => &mut self.python,
            _ => return Ok(skipped(file_record, FileStatus::SkippedExtension)),
        };

        let parsed_source =
            ParsedSource::parse(lang, file_id, entry.relative_path.clone(), source)?;

        let t_rules = std::time::Instant::now();
        let findings = self.rules.run(file_id, &parsed_source);
        let rules_ms = t_rules.elapsed().as_millis() as u64;

        let parsed: ParsedFile = extractor.extract(&parsed_source);

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

fn skipped(file_record: FileRecord, status: FileStatus) -> ProcessOutcome {
    ProcessOutcome {
        file_record: FileRecord { status, ..file_record },
        parsed: None,
        findings: Vec::new(),
        timing: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleSet;
    use crate::walk::WalkEntry;
    use tempfile::TempDir;

    fn entry(dir: &TempDir, name: &str, contents: &[u8], language: Option<&str>) -> WalkEntry {
        let path = dir.path().join(name);
        std::fs::write(&path, contents).unwrap();
        WalkEntry {
            path,
            relative_path: name.to_string(),
            size_bytes: contents.len() as u64,
            language: language.map(str::to_string),
            status: FileStatus::Indexed,
        }
    }

    #[test]
    fn indexed_rust_file_produces_symbols_and_timing() {
        let dir = TempDir::new().unwrap();
        let e = entry(&dir, "x.rs", b"pub fn greet() {}\n", Some("rust"));
        let mut fp = FileProcessor::new(RuleSet::empty()).unwrap();
        let out = fp.process(1, &e).unwrap();
        let parsed = out.parsed.expect("indexed file should produce parsed output");
        assert!(parsed.symbols.iter().any(|s| s.qualified_name == "greet"));
        assert!(out.timing.is_some());
        assert_eq!(out.file_record.status, FileStatus::Indexed);
    }

    #[test]
    fn non_indexed_status_passes_through() {
        let dir = TempDir::new().unwrap();
        let mut e = entry(&dir, "big.rs", b"pub fn x() {}\n", Some("rust"));
        e.status = FileStatus::SkippedSize;
        let mut fp = FileProcessor::new(RuleSet::empty()).unwrap();
        let out = fp.process(1, &e).unwrap();
        assert!(out.parsed.is_none());
        assert!(out.findings.is_empty());
        assert!(out.timing.is_none());
        assert_eq!(out.file_record.status, FileStatus::SkippedSize);
    }

    #[test]
    fn non_utf8_bytes_demote_to_skipped_binary() {
        let dir = TempDir::new().unwrap();
        let e = entry(&dir, "x.rs", &[0xff, 0xfe, 0x00, 0x01], Some("rust"));
        let mut fp = FileProcessor::new(RuleSet::empty()).unwrap();
        let out = fp.process(1, &e).unwrap();
        assert_eq!(out.file_record.status, FileStatus::SkippedBinary);
        assert!(out.parsed.is_none());
    }

    #[test]
    fn unknown_language_demotes_to_skipped_extension() {
        let dir = TempDir::new().unwrap();
        let e = entry(&dir, "x.cobol", b"PROGRAM-ID. NOTHING.\n", Some("cobol"));
        let mut fp = FileProcessor::new(RuleSet::empty()).unwrap();
        let out = fp.process(1, &e).unwrap();
        assert_eq!(out.file_record.status, FileStatus::SkippedExtension);
        assert!(out.parsed.is_none());
    }

    #[test]
    fn no_language_string_demotes_to_skipped_extension() {
        let dir = TempDir::new().unwrap();
        let e = entry(&dir, "weird", b"x\n", None);
        let mut fp = FileProcessor::new(RuleSet::empty()).unwrap();
        let out = fp.process(1, &e).unwrap();
        assert_eq!(out.file_record.status, FileStatus::SkippedExtension);
    }

}
