use tracing::{info, info_span, warn};

use crate::core::{
    BuildRequest, BuildSummary, Counters, EdgeKind, FileStatus, FileTiming, INDEXER_VERSION,
    MallardError, Metadata, Result,
};
use crate::file_processor::FileProcessor;
use crate::rules::RuleSet;
use crate::store::IndexWriter;
use crate::walk::{self, WalkOptions};

const EDGE_KINDS_FOR_COUNTERS: &[EdgeKind] = &[
    EdgeKind::Calls,
    EdgeKind::Imports,
    EdgeKind::Contains,
];

pub fn build(req: BuildRequest) -> Result<BuildSummary> {
    let started = std::time::Instant::now();
    let _root_span = info_span!("build", sha = %req.sha).entered();

    let rules = match req.rules_path.as_ref() {
        Some(p) => RuleSet::load(p)?,
        None => RuleSet::empty(),
    };

    let mut processor = FileProcessor::new(rules)?;

    let metadata = Metadata {
        sha: req.sha.clone(),
        indexer_version: INDEXER_VERSION.to_string(),
        rule_set_hash: processor.rule_set_hash().map(str::to_string),
        built_at: current_timestamp(),
        language_allow_list: req.language_allow_list.clone(),
    };

    let mut writer = IndexWriter::create(&req.out_path, &metadata)?;

    let root = req.root.canonicalize().map_err(|e| {
        MallardError::InvalidPath(format!("could not canonicalize {}: {e}", req.root.display()))
    })?;
    let walk_entries = walk::walk(
        &root,
        &WalkOptions {
            max_file_bytes: req.max_file_bytes,
            language_allow_list: req.language_allow_list.clone(),
        },
    );
    info!(entries = walk_entries.len(), "walk complete");

    let mut counters = Counters::default();
    let mut file_timings: Vec<FileTiming> = Vec::new();

    for (idx, entry) in walk_entries.iter().enumerate() {
        let file_id = (idx as i64) + 1;
        match processor.process(file_id, entry) {
            Ok(outcome) => {
                match (&outcome.file_record.status, &outcome.parsed) {
                    (FileStatus::Indexed, Some(parsed)) => {
                        writer.write_indexed(&outcome.file_record, parsed, &outcome.findings)?;
                    }
                    _ => {
                        writer.write_skipped(&outcome.file_record)?;
                    }
                }
                if let Some(timing) = &outcome.timing {
                    file_timings.push(timing.clone());
                }
                counters.record(&outcome);
            }
            Err(e) => {
                warn!(path = %entry.relative_path, error = %e, "file processing failed");
            }
        }
    }

    let resolve_stats = writer.resolve_edges()?;
    info!(
        inspected = resolve_stats.inspected,
        resolved = resolve_stats.resolved,
        "edge resolution complete"
    );

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

fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}
