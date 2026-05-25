//! Deterministic-only PR review pipeline (no LLM). Reads a base + head
//! index plus a list of changed files, computes the structural-evidence
//! delta, and emits comments tagged by confidence tier. Designed to be
//! consumed by a GitHub Action wrapper that posts inline comments via
//! `gh pr review`. The LLM-synthesis layer ships in Phase D — this stage
//! is the trust-calibrated baseline that fits the privacy wedge: no code
//! leaves the runner.
//!
//! Pipeline shape (per `docs/specs/pr-review/pull-request-review.md`):
//!   - Stage 3: signature diff (added / removed symbols)
//!   - Stage 4: outbound-edge diff per stable symbol (`modified-body`)
//!   - Stage 5 (deterministic only): findings in head index for changed
//!     files → emit as `structural-rule` comments
//!   - Stage 7: cap and label
//!
//! ADR-0010 tier surfaces verbatim on each comment: reviewers filter to
//! `extracted` only on a noisy PR or expand to `ambiguous` on demand.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::{Result, SymbolId};
use crate::query::{Direction, FindingFilter, IndexReader, NeighborEdge, SymbolRecord};
use crate::EdgeKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrReviewRequest {
    pub base_db: PathBuf,
    pub head_db: PathBuf,
    pub changed_files: Vec<String>,
    pub max_comments: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrReviewResult {
    pub summary: PrReviewSummary,
    pub comments: Vec<ReviewComment>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrReviewSummary {
    pub files_in_scope: usize,
    pub symbols_added: usize,
    pub symbols_removed: usize,
    pub symbols_modified_body: usize,
    pub comments_emitted: usize,
    pub comments_dropped_to_budget: usize,
    pub by_source_kind: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewComment {
    pub file: String,
    pub line: u32,
    pub end_line: u32,
    pub symbol_qualified_name: Option<String>,
    pub symbol_id: Option<String>,
    pub source_kind: String,
    pub confidence_tier: String,
    pub rule_id: Option<String>,
    pub body: String,
}

pub fn run(req: PrReviewRequest) -> Result<PrReviewResult> {
    let base = IndexReader::open(&req.base_db)?;
    let head = IndexReader::open(&req.head_db)?;
    let mut comments: Vec<ReviewComment> = Vec::new();
    let mut summary = PrReviewSummary::default();
    summary.files_in_scope = req.changed_files.len();

    for path in &req.changed_files {
        review_file(&base, &head, path, &mut comments, &mut summary)?;
    }

    let total = comments.len();
    if total > req.max_comments {
        // Drop lowest-tier first: structural-rule > extracted > inferred >
        // ambiguous → keep the highest-signal slice.
        comments.sort_by_key(|c| tier_priority(&c.confidence_tier));
        comments.truncate(req.max_comments);
        summary.comments_dropped_to_budget = total - req.max_comments;
    }
    summary.comments_emitted = comments.len();
    for c in &comments {
        *summary
            .by_source_kind
            .entry(c.source_kind.clone())
            .or_default() += 1;
    }
    Ok(PrReviewResult { summary, comments })
}

fn review_file(
    base: &IndexReader,
    head: &IndexReader,
    path: &str,
    out: &mut Vec<ReviewComment>,
    summary: &mut PrReviewSummary,
) -> Result<()> {
    // Stage 5 — structural-rule findings on the head SHA, scoped to the
    // changed file. Highest-trust tier (deterministic rule match).
    let findings = head.findings(&FindingFilter {
        path_prefix: Some(path.to_string()),
        ..Default::default()
    })?;
    for f in findings {
        out.push(ReviewComment {
            file: f.path.clone(),
            line: f.start_line,
            end_line: f.end_line,
            symbol_qualified_name: None,
            symbol_id: None,
            source_kind: "structural-rule".to_string(),
            confidence_tier: "structural-rule".to_string(),
            rule_id: Some(f.rule_id),
            body: f.message,
        });
    }

    // Stage 3 — signature diff. For each changed file, compute added /
    // removed symbol IDs across the two indexes. Modified-signature is
    // pair-matched by qualified_name within a file (one added + one
    // removed sharing a name).
    let base_syms = base.symbols_in_file(path).unwrap_or_default();
    let head_syms = head.symbols_in_file(path).unwrap_or_default();
    let base_ids: HashMap<String, SymbolRecord> = base_syms
        .iter()
        .cloned()
        .map(|s| (s.id.0.clone(), s))
        .collect();
    let head_ids: HashMap<String, SymbolRecord> = head_syms
        .iter()
        .cloned()
        .map(|s| (s.id.0.clone(), s))
        .collect();
    let added: Vec<&SymbolRecord> = head_syms
        .iter()
        .filter(|s| !base_ids.contains_key(&s.id.0))
        .collect();
    let removed: Vec<&SymbolRecord> = base_syms
        .iter()
        .filter(|s| !head_ids.contains_key(&s.id.0))
        .collect();
    summary.symbols_added += added.len();
    summary.symbols_removed += removed.len();

    // Stage 4 — outbound-edge diff per stable-ID symbol. Modified-body
    // comments cite the changed callee names. Pure-logic body changes
    // (callee set unchanged but anchor byte-span changed → lines added /
    // removed inside the body) emit a separate `modified-body-logic`
    // signal at `inferred` tier. Pilot Finding 4.
    let stable_ids: Vec<&SymbolRecord> = head_syms
        .iter()
        .filter(|s| base_ids.contains_key(&s.id.0))
        .collect();
    for sym in stable_ids {
        let head_calls = collect_outbound_callee_names(head, &sym.id);
        let base_calls = collect_outbound_callee_names(base, &sym.id);
        let added_callees: Vec<String> = head_calls
            .iter()
            .filter(|n| !base_calls.contains(n))
            .cloned()
            .collect();
        let removed_callees: Vec<String> = base_calls
            .iter()
            .filter(|n| !head_calls.contains(n))
            .cloned()
            .collect();

        if !added_callees.is_empty() || !removed_callees.is_empty() {
            summary.symbols_modified_body += 1;
            let body = render_modified_body_comment(&sym.qualified_name, &added_callees, &removed_callees);
            let tier = if head_calls.iter().all(|c| !c.starts_with('[')) {
                "extracted"
            } else {
                "inferred"
            };
            out.push(ReviewComment {
                file: sym.path.clone(),
                line: sym.anchor.start_line,
                end_line: sym.anchor.end_line,
                symbol_qualified_name: Some(sym.qualified_name.clone()),
                symbol_id: Some(sym.id.0.clone()),
                source_kind: "modified-body".to_string(),
                confidence_tier: tier.to_string(),
                rule_id: None,
                body,
            });
            continue;
        }

        // Pure-logic body change: callee set identical but anchor byte
        // span changed → lines added / removed inside the body without
        // adding new outbound calls. Anchor end_line shifts too, so use
        // both base + head anchors. The `inferred` tier signals "we know
        // the body changed; we don't know the semantic impact."
        let base_sym = base_ids.get(&sym.id.0).expect("stable-id lookup");
        let head_span = sym.anchor.end_byte.saturating_sub(sym.anchor.start_byte);
        let base_span = base_sym.anchor.end_byte.saturating_sub(base_sym.anchor.start_byte);
        if head_span != base_span {
            summary.symbols_modified_body += 1;
            out.push(ReviewComment {
                file: sym.path.clone(),
                line: sym.anchor.start_line,
                end_line: sym.anchor.end_line,
                symbol_qualified_name: Some(sym.qualified_name.clone()),
                symbol_id: Some(sym.id.0.clone()),
                source_kind: "modified-body-logic".to_string(),
                confidence_tier: "inferred".to_string(),
                rule_id: None,
                body: format!(
                    "`{}` body length changed ({} → {} bytes) with the outbound call set intact. \
                    Pure-logic / control-flow change — manual review recommended.",
                    sym.qualified_name, base_span, head_span
                ),
            });
        }
    }
    Ok(())
}

fn collect_outbound_callee_names(reader: &IndexReader, id: &SymbolId) -> Vec<String> {
    let edges: Vec<NeighborEdge> = reader
        .neighbors(id, &[EdgeKind::Calls], Direction::Out)
        .unwrap_or_default();
    let mut names: Vec<String> = edges
        .iter()
        .map(|e| match (&e.dst, &e.dst_unresolved) {
            (Some(d), _) => d.qualified_name.clone(),
            (None, Some(name)) => format!("[{name}]"),
            (None, None) => String::new(),
        })
        .filter(|n| !n.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

fn render_modified_body_comment(
    qualified_name: &str,
    added: &[String],
    removed: &[String],
) -> String {
    let mut buf = format!("`{qualified_name}` outbound-call set changed.\n");
    if !added.is_empty() {
        buf.push_str("\n**+ Added callees:**\n");
        for c in added {
            buf.push_str(&format!("- `{c}`\n"));
        }
    }
    if !removed.is_empty() {
        buf.push_str("\n**− Removed callees:**\n");
        for c in removed {
            buf.push_str(&format!("- `{c}`\n"));
        }
    }
    buf
}

fn tier_priority(tier: &str) -> u32 {
    // Lower number = higher priority (kept under budget). Sort key for
    // comment-budget truncation when over `max_comments`.
    match tier {
        "structural-rule" => 0,
        "extracted" => 1,
        "inferred" => 2,
        "ambiguous" => 3,
        _ => 4,
    }
}

pub fn render_markdown(result: &PrReviewResult) -> String {
    let mut buf = String::new();
    buf.push_str("# mallard PR review\n\n");
    buf.push_str(&format!(
        "Files in scope: {} · symbols +{}/−{} · modified-body {} · comments {} (dropped {})\n\n",
        result.summary.files_in_scope,
        result.summary.symbols_added,
        result.summary.symbols_removed,
        result.summary.symbols_modified_body,
        result.summary.comments_emitted,
        result.summary.comments_dropped_to_budget,
    ));
    for c in &result.comments {
        let badge = format!("[{}]", c.confidence_tier);
        let rule = c
            .rule_id
            .as_ref()
            .map(|r| format!(" · `{r}`"))
            .unwrap_or_default();
        buf.push_str(&format!(
            "### {} · `{}:{}` {}\n{}\n\n{}\n\n",
            badge, c.file, c.line, rule, c.source_kind, c.body
        ));
    }
    buf
}

/// Helper for tests / CLI callers that have file paths but no env.
pub fn from_paths(
    base_db: &Path,
    head_db: &Path,
    changed_files: Vec<String>,
    max_comments: usize,
) -> Result<PrReviewResult> {
    run(PrReviewRequest {
        base_db: base_db.to_path_buf(),
        head_db: head_db.to_path_buf(),
        changed_files,
        max_comments,
    })
}
