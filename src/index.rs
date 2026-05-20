use tracing::{info, info_span, warn};

use crate::core::{
    BuildRequest, BuildSummary, Counters, EdgeKind, FileId, FileRecord, FileStatus, FileTiming,
    INDEXER_VERSION, MallardError, Metadata, Result,
};
use crate::parser::{ParsedFile, RustParser};
use crate::rules::RuleSet;
use crate::store::IndexWriter;
use crate::walk::{self, WalkEntry, WalkOptions};

const EDGE_KINDS_FOR_COUNTERS: &[EdgeKind] = &[
    EdgeKind::Calls,
    EdgeKind::Imports,
    EdgeKind::Contains,
    EdgeKind::References,
];

pub fn build(req: BuildRequest) -> Result<BuildSummary> {
    let started = std::time::Instant::now();
    let _root_span = info_span!("build", sha = %req.sha).entered();

    let rules = match req.rules_path.as_ref() {
        Some(p) => RuleSet::load(p)?,
        None => RuleSet::empty(),
    };

    let metadata = Metadata {
        sha: req.sha.clone(),
        indexer_version: INDEXER_VERSION.to_string(),
        rule_set_hash: rules.source_hash.clone(),
        built_at: current_timestamp(),
        language_allow_list: req.language_allow_list.clone(),
    };

    let mut writer = IndexWriter::create(&req.out_path, &metadata)?;

    let walk_opts = WalkOptions {
        max_file_bytes: req.max_file_bytes,
        language_allow_list: req.language_allow_list.clone(),
    };
    let root = req.root.canonicalize().map_err(|e| {
        MallardError::InvalidPath(format!("could not canonicalize {}: {e}", req.root.display()))
    })?;
    let walk_entries = walk::walk(&root, &walk_opts);
    info!(entries = walk_entries.len(), "walk complete");

    let mut counters = Counters::default();
    let mut file_timings: Vec<FileTiming> = Vec::new();

    let mut parser = RustParser::new()?;

    for (idx, entry) in walk_entries.iter().enumerate() {
        let file_id: FileId = (idx as i64) + 1;
        let file_record = FileRecord {
            id: file_id,
            path: entry.relative_path.clone(),
            language: entry.language.clone(),
            size_bytes: entry.size_bytes,
            status: entry.status,
        };
        writer.append_file(&file_record)?;

        match entry.status {
            FileStatus::Indexed => {
                if let Err(e) = process_indexed_file(
                    file_id,
                    entry,
                    &mut parser,
                    &rules,
                    &mut writer,
                    &mut counters,
                    &mut file_timings,
                ) {
                    warn!(path = %entry.relative_path, error = %e, "file processing failed");
                }
            }
            FileStatus::Unparseable => {
                counters.parse_errors += 1;
            }
            other => {
                *counters
                    .files_skipped_by_reason
                    .entry(other.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }
    }

    writer.finalize()?;

    for kind in EDGE_KINDS_FOR_COUNTERS {
        counters
            .edges_by_kind
            .entry(kind.as_str().to_string())
            .or_insert(0);
    }

    file_timings.sort_by(|a, b| {
        (b.parse_ms + b.query_ms + b.rules_ms).cmp(&(a.parse_ms + a.query_ms + a.rules_ms))
    });
    file_timings.truncate(req.slowest_files_n);

    Ok(BuildSummary {
        sha: req.sha,
        indexer_version: INDEXER_VERSION.to_string(),
        rule_set_hash: metadata.rule_set_hash.clone(),
        out_path: req.out_path,
        elapsed_ms: started.elapsed().as_millis() as u64,
        counters,
        slowest_files: file_timings,
    })
}

fn process_indexed_file(
    file_id: FileId,
    entry: &WalkEntry,
    parser: &mut RustParser,
    rules: &RuleSet,
    writer: &mut IndexWriter,
    counters: &mut Counters,
    timings: &mut Vec<FileTiming>,
) -> Result<()> {
    let source = std::fs::read(&entry.path)?;
    let language = entry.language.as_deref().unwrap_or_default();

    let t_rules = std::time::Instant::now();
    let findings = rules.run(file_id, &source, language);
    let rules_ms = t_rules.elapsed().as_millis() as u64;

    let parsed: ParsedFile = parser.parse_file(file_id, &entry.relative_path, source)?;

    writer.append_parse_errors(&parsed.parse_errors)?;
    counters.parse_errors += parsed.parse_errors.len() as u64;

    writer.append_symbols(file_id, &parsed.symbols)?;
    counters.symbols += parsed.symbols.len() as u64;

    writer.append_edges(&parsed.edges)?;
    for edge in &parsed.edges {
        *counters
            .edges_by_kind
            .entry(edge.kind.as_str().to_string())
            .or_insert(0) += 1;
    }

    writer.append_findings(&findings)?;
    counters.findings += findings.len() as u64;

    if parsed.parse_errors.is_empty() {
        counters.files_indexed += 1;
    }

    timings.push(FileTiming {
        path: entry.relative_path.clone(),
        parse_ms: parsed.parse_ms,
        query_ms: parsed.query_ms,
        rules_ms,
    });

    Ok(())
}

fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}
