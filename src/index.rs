use std::collections::BTreeMap;

use tracing::{info, info_span, warn};

use crate::core::{
    BuildRequest, BuildSummary, Counters, FileId, FileRecord, FileStatus, FileTiming,
    INDEXER_VERSION, MallardError, Metadata, Result,
};
use crate::parser::{ParsedFile, RustParser};
use crate::rules::RuleSet;
use crate::store::IndexWriter;
use crate::walk::{self, WalkEntry, WalkOptions};

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
                    counters.parse_errors += 1;
                }
            }
            FileStatus::Unparseable => {
                counters.parse_errors += 1;
            }
            other => {
                let reason = other.as_str().to_string();
                *counters.files_skipped_by_reason.entry(reason).or_insert(0) += 1;
            }
        }
    }

    writer.finalize()?;

    counters
        .edges_by_kind
        .entry("calls".to_string())
        .or_insert(0);
    counters
        .edges_by_kind
        .entry("imports".to_string())
        .or_insert(0);
    counters
        .edges_by_kind
        .entry("contains".to_string())
        .or_insert(0);
    counters
        .edges_by_kind
        .entry("references".to_string())
        .or_insert(0);

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
    let language = entry.language.as_deref().unwrap_or("rust");

    let t_rules = std::time::Instant::now();
    let findings = rules.run(file_id, &source, language);
    let rules_ms = t_rules.elapsed().as_millis() as u64;

    let parsed: ParsedFile = parser.parse_file(file_id, &entry.relative_path, source)?;

    for pe in &parsed.parse_errors {
        writer.append_parse_error(pe)?;
        counters.parse_errors += 1;
    }

    let mut edge_kind_counts: BTreeMap<String, u64> = BTreeMap::new();

    for sym in &parsed.symbols {
        writer.append_symbol(file_id, sym)?;
        counters.symbols += 1;
    }

    for edge in &parsed.edges {
        writer.append_edge(edge)?;
        *edge_kind_counts
            .entry(edge.kind.as_str().to_string())
            .or_insert(0) += 1;
    }

    for finding in &findings {
        writer.append_finding(finding)?;
        counters.findings += 1;
    }

    if parsed.parse_errors.is_empty() {
        counters.files_indexed += 1;
    }

    for (k, v) in edge_kind_counts {
        *counters.edges_by_kind.entry(k).or_insert(0) += v;
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
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}
