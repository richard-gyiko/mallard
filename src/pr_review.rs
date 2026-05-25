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

use crate::core::{Result, SymbolId, SymbolKind};
use crate::query::{Direction, FindingFilter, IndexReader, NeighborEdge, SymbolRecord};
use crate::EdgeKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrReviewRequest {
    pub base_db: PathBuf,
    pub head_db: PathBuf,
    pub changed_files: Vec<String>,
    pub max_comments: usize,
    /// Optional per-file diff hunks (1-based line ranges in the head
    /// SHA's file content). When provided, enables Finding-7 detection:
    /// stable-ID symbols whose anchor overlaps a diff hunk get a
    /// `modified-body-touched` comment even when callees + byte-span
    /// don't change. Source: `git diff --unified=0` post-processed by
    /// the caller into JSON. See `docs/research/finding-7-...md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_hunks: Option<DiffHunks>,
    /// Pattern C (M4 grading): suppress `modified-body-touched` comments
    /// in test files when the diff hunks cover only 1-2 lines. Test
    /// fixtures often flip literal values (`262142` → `262146`) — the
    /// touched-body signal is structurally correct but adds no review
    /// value. Default `false` keeps the signal on for callers who want
    /// every overlap surfaced.
    #[serde(default)]
    pub ignore_test_trivia: bool,
}

/// Per-file diff hunks consumed by the review pipeline.
///
/// JSON shape:
/// ```json
/// { "files": { "lib/foo.rs": [{"start": 42, "end": 58}, ...] } }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffHunks {
    pub files: HashMap<String, Vec<DiffRange>>,
}

/// 1-based inclusive line range identifying a changed region.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DiffRange {
    pub start: u32,
    pub end: u32,
}

impl DiffRange {
    fn overlaps(&self, anchor_start: u32, anchor_end: u32) -> bool {
        self.start <= anchor_end && self.end >= anchor_start
    }
}

/// Parse the output of `git diff --unified=0 <base> <head>` into per-file
/// diff hunks. Each `@@ -A,B +C,D @@` header contributes one DiffRange
/// keyed by the b-side file name (the head-SHA path).
///
/// Robust to:
/// - Single-line hunks (`+C` with no count → count = 1)
/// - Deletion-only hunks (`+C,0` → skipped; no head-side lines)
/// - Rename headers (`diff --git a/old b/new` → key = `new`)
pub fn parse_unified_diff_hunks(diff_text: &str) -> DiffHunks {
    let mut files: HashMap<String, Vec<DiffRange>> = HashMap::new();
    let mut current_file: Option<String> = None;
    for line in diff_text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // `a/path b/path` — take the b-side as the head-SHA path.
            if let Some(b_idx) = rest.find(" b/") {
                current_file = Some(rest[b_idx + 3..].to_string());
            } else {
                current_file = None;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("+++ b/") {
            // `+++ b/<path>` — preferred source for the file name when
            // present; overrides the `diff --git` guess for cases where
            // the path contains spaces.
            current_file = Some(rest.to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            // `@@ -A,B +C,D @@ context...`
            let plus_idx = match rest.find('+') {
                Some(i) => i,
                None => continue,
            };
            let after_plus = &rest[plus_idx + 1..];
            let end = after_plus
                .find(|c: char| c == ' ' || c == '@')
                .unwrap_or(after_plus.len());
            let plus_part = &after_plus[..end];
            let (start_str, count_str) = match plus_part.split_once(',') {
                Some((s, c)) => (s, c),
                None => (plus_part, "1"),
            };
            let start: u32 = match start_str.parse() {
                Ok(n) => n,
                Err(_) => continue,
            };
            let count: u32 = count_str.parse().unwrap_or(1);
            if count == 0 {
                continue;
            }
            let end_line = start + count - 1;
            if let Some(file) = current_file.as_ref() {
                files.entry(file.clone()).or_default().push(DiffRange {
                    start,
                    end: end_line,
                });
            }
        }
    }
    DiffHunks { files }
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

/// Internal-only carrier: review comment plus the metadata needed to
/// apply post-processing filters (container suppression, diff overlap).
/// Not serialised — public `ReviewComment` stays stable.
struct PendingComment {
    comment: ReviewComment,
    /// `None` for structural-rule findings (no enclosing symbol tracked).
    symbol_kind: Option<SymbolKind>,
    /// Byte span of the enclosing symbol (modified-body family only).
    /// `None` for structural-rule comments.
    anchor: Option<(u64, u64)>,
}

pub fn run(req: PrReviewRequest) -> Result<PrReviewResult> {
    let base = IndexReader::open(&req.base_db)?;
    let head = IndexReader::open(&req.head_db)?;
    let mut summary = PrReviewSummary::default();
    summary.files_in_scope = req.changed_files.len();
    let empty_hunks: Vec<DiffRange> = Vec::new();

    let mut pending: Vec<PendingComment> = Vec::new();
    for path in &req.changed_files {
        let hunks = req
            .diff_hunks
            .as_ref()
            .and_then(|h| h.files.get(path))
            .map(Vec::as_slice)
            .unwrap_or(empty_hunks.as_slice());
        review_file(
            &base,
            &head,
            path,
            hunks,
            req.ignore_test_trivia,
            &mut pending,
            &mut summary,
        )?;
    }

    // Pattern A — container restate suppression. A comment on a
    // container kind (class / struct / interface / type alias / module)
    // is redundant when one of its enclosed leaf symbols also emits in
    // the same file. Drop the container comment.
    suppress_container_restate(&mut pending);

    // Pattern A2 (M4 grading follow-up) — function-in-function restate
    // suppression. When a Function/Method's anchor strictly encloses
    // another emitted Function/Method's anchor in the same file, the
    // outer's comment restates "this function was edited" without the
    // precision of the inner's. Targets the axios shape where a named
    // outer fn-expr (`httpAdapter`) wraps the substantive inner fn
    // (`dispatchHttpRequest`).
    suppress_outer_function_restate(&mut pending);

    let mut comments: Vec<ReviewComment> = pending.into_iter().map(|p| p.comment).collect();
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

/// Pattern A: drop comments on container kinds whose anchor strictly
/// encloses another comment's anchor in the same file. The leaf comment
/// is the higher-signal observation; the container's "body length
/// changed" framing duplicates it.
fn suppress_container_restate(pending: &mut Vec<PendingComment>) {
    let mut to_drop: Vec<usize> = Vec::new();
    for (i, c) in pending.iter().enumerate() {
        if !is_container_kind(c.symbol_kind) {
            continue;
        }
        let Some((cstart, cend)) = c.anchor else {
            continue;
        };
        let encloses_leaf = pending.iter().enumerate().any(|(j, other)| {
            if i == j || other.comment.file != c.comment.file {
                return false;
            }
            // Only count NON-container leaves as suppression triggers —
            // two nested containers shouldn't cancel each other.
            if is_container_kind(other.symbol_kind) {
                return false;
            }
            let Some((ostart, oend)) = other.anchor else {
                return false;
            };
            ostart > cstart && oend < cend
        });
        if encloses_leaf {
            to_drop.push(i);
        }
    }
    // Drop in reverse so indices stay valid.
    for &i in to_drop.iter().rev() {
        pending.remove(i);
    }
}

fn is_container_kind(kind: Option<SymbolKind>) -> bool {
    matches!(
        kind,
        Some(
            SymbolKind::Struct
                | SymbolKind::Trait
                | SymbolKind::TypeAlias
                | SymbolKind::Module
                | SymbolKind::Enum
        )
    )
}

/// Pattern A2: drop a Function / Method comment whose anchor strictly
/// encloses another emitted Function / Method comment in the same file.
/// The outer fn restates "this function was edited"; the inner fn's
/// comment carries the precise observation. Both must be Fn-family;
/// containers are handled by Pattern A.
fn suppress_outer_function_restate(pending: &mut Vec<PendingComment>) {
    let mut to_drop: Vec<usize> = Vec::new();
    for (i, outer) in pending.iter().enumerate() {
        if !is_fn_family(outer.symbol_kind) {
            continue;
        }
        let Some((outer_start, outer_end)) = outer.anchor else {
            continue;
        };
        let encloses_inner_fn = pending.iter().enumerate().any(|(j, inner)| {
            if i == j || inner.comment.file != outer.comment.file {
                return false;
            }
            if !is_fn_family(inner.symbol_kind) {
                return false;
            }
            let Some((inner_start, inner_end)) = inner.anchor else {
                return false;
            };
            inner_start > outer_start && inner_end < outer_end
        });
        if encloses_inner_fn {
            to_drop.push(i);
        }
    }
    for &i in to_drop.iter().rev() {
        pending.remove(i);
    }
}

fn is_fn_family(kind: Option<SymbolKind>) -> bool {
    matches!(kind, Some(SymbolKind::Function | SymbolKind::Method))
}

/// Pattern C helper: classify a symbol as "test-shaped" via either its
/// file path OR its qualified-name prefix. Rust's idiomatic `mod tests
/// { #[test] fn binary3() ... }` puts tests inside non-test-named
/// files (`crates/searcher/src/searcher/glue.rs`), so the path alone
/// misses them — the qualified name starts with `tests::`.
pub(crate) fn is_test_symbol(path: &str, qualified_name: Option<&str>) -> bool {
    if is_test_path(path) {
        return true;
    }
    if let Some(qname) = qualified_name {
        // Rust convention: `mod tests { fn foo() {} }` → `tests::foo`.
        // Python: `class TestFoo` or `def test_foo` at module level
        // → `TestFoo.method` or just `test_foo`.
        if qname.starts_with("tests::")
            || qname.starts_with("tests.")
            || qname.starts_with("test::")
            || qname.starts_with("test.")
        {
            return true;
        }
        // Fn names like `test_*` (Python convention) or symbols inside
        // a Test*-prefixed class.
        let last = qname.rsplit_once("::").map(|(_, t)| t).unwrap_or(qname);
        let last = last.rsplit_once('.').map(|(_, t)| t).unwrap_or(last);
        if last.starts_with("test_") {
            return true;
        }
    }
    false
}

/// Pattern C: classify a path as test-shaped via filename / directory
/// conventions across Rust, Python, JS/TS. Conservative — matches the
/// common patterns; misses exotic naming but the cost is a stray
/// test-trivia comment, not a wrong claim.
pub(crate) fn is_test_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    // Directory conventions: tests/ at root or nested.
    if lower.starts_with("tests/")
        || lower.starts_with("test/")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.contains("/__tests__/")
    {
        return true;
    }
    // Filename conventions.
    if lower.ends_with("_test.rs")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".test.js")
        || lower.ends_with(".test.jsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".spec.tsx")
        || lower.ends_with(".spec.js")
        || lower.ends_with(".spec.jsx")
    {
        return true;
    }
    // Python test fixture conventions (test_*.py or *_test.py).
    if let Some(filename) = lower.rsplit('/').next() {
        if filename.starts_with("test_") && filename.ends_with(".py") {
            return true;
        }
        if filename.ends_with("_test.py") {
            return true;
        }
    }
    false
}

/// Pattern D: only emit modified-body-logic for callable kinds. Interfaces,
/// type aliases, consts, statics carry no "body" in the function sense —
/// their span-delta framing misleads reviewers.
fn is_callable_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Macro
    )
}

fn review_file(
    base: &IndexReader,
    head: &IndexReader,
    path: &str,
    diff_hunks: &[DiffRange],
    ignore_test_trivia: bool,
    out: &mut Vec<PendingComment>,
    summary: &mut PrReviewSummary,
) -> Result<()> {
    // Stage 5 — structural-rule findings on the head SHA, scoped to the
    // changed file. Highest-trust tier (deterministic rule match).
    //
    // Pattern B (M4 grading fix): when the caller supplied diff hunks,
    // require the finding's line to overlap a hunk. Without overlap the
    // rule is firing on pre-existing code untouched by the PR, which
    // dominated the noise in pilot v4 (e.g. requests #7433 hit 7
    // pickle.loads sites in tests/ that long predated the PR). If no
    // diff hunks are supplied, fall back to current behaviour for
    // backward compatibility.
    let rule_overlap_required = !diff_hunks.is_empty();
    let findings = head.findings(&FindingFilter {
        path_prefix: Some(path.to_string()),
        ..Default::default()
    })?;
    for f in findings {
        if rule_overlap_required
            && !diff_hunks
                .iter()
                .any(|r| r.overlaps(f.start_line, f.end_line))
        {
            continue;
        }
        out.push(PendingComment {
            comment: ReviewComment {
                file: f.path.clone(),
                line: f.start_line,
                end_line: f.end_line,
                symbol_qualified_name: None,
                symbol_id: None,
                source_kind: "structural-rule".to_string(),
                confidence_tier: "structural-rule".to_string(),
                rule_id: Some(f.rule_id),
                body: f.message,
            },
            symbol_kind: None,
            anchor: None,
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

        let anchor = (sym.anchor.start_byte, sym.anchor.end_byte);

        if !added_callees.is_empty() || !removed_callees.is_empty() {
            summary.symbols_modified_body += 1;
            let body = render_modified_body_comment(&sym.qualified_name, &added_callees, &removed_callees);
            let tier = if head_calls.iter().all(|c| !c.starts_with('[')) {
                "extracted"
            } else {
                "inferred"
            };
            out.push(PendingComment {
                comment: ReviewComment {
                    file: sym.path.clone(),
                    line: sym.anchor.start_line,
                    end_line: sym.anchor.end_line,
                    symbol_qualified_name: Some(sym.qualified_name.clone()),
                    symbol_id: Some(sym.id.0.clone()),
                    source_kind: "modified-body".to_string(),
                    confidence_tier: tier.to_string(),
                    rule_id: None,
                    body,
                },
                symbol_kind: Some(sym.kind),
                anchor: Some(anchor),
            });
            continue;
        }

        // Pure-logic body change: callee set identical but anchor byte
        // span changed → lines added / removed inside the body without
        // adding new outbound calls. The `inferred` tier signals "we
        // know the body changed; we don't know the semantic impact."
        //
        // Pattern D (M4 grading fix): restrict to callable kinds.
        // Interfaces, type aliases, consts, statics carry no "body" in
        // the function sense — emitting on them misleads reviewers
        // (e.g. axios #10920 flagged `TransitionalOptions` interface
        // because a field was added).
        let base_sym = base_ids.get(&sym.id.0).expect("stable-id lookup");
        let head_span = sym.anchor.end_byte.saturating_sub(sym.anchor.start_byte);
        let base_span = base_sym.anchor.end_byte.saturating_sub(base_sym.anchor.start_byte);
        if head_span != base_span && is_callable_kind(sym.kind) {
            summary.symbols_modified_body += 1;
            out.push(PendingComment {
                comment: ReviewComment {
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
                },
                symbol_kind: Some(sym.kind),
                anchor: Some(anchor),
            });
            continue;
        }

        // Finding-7 signal — `modified-body-touched`. Callees + byte-span
        // both unchanged, but the PR diff hunks landed lines inside the
        // symbol's anchor range. Caller must supply diff hunks for this
        // branch to fire (no overlap when `diff_hunks` is empty); the
        // GitHub Action computes them via `git diff --unified=0` →
        // `mallard diff-hunks`. Tier `inferred` — we know the lines
        // moved; we don't know the semantic impact.
        //
        // Pattern D applies here too — keep the signal scoped to
        // callable kinds; module-level interfaces / consts already
        // surface as added/removed elsewhere.
        if !diff_hunks.is_empty() && is_callable_kind(sym.kind) {
            let overlaps: Vec<DiffRange> = diff_hunks
                .iter()
                .copied()
                .filter(|r| r.overlaps(sym.anchor.start_line + 1, sym.anchor.end_line + 1))
                .collect();
            if !overlaps.is_empty() {
                // Pattern C: in test files, suppress when the overlap
                // covers ≤2 lines. Test fixtures routinely flip a
                // literal value or assertion expectation; the touched
                // signal is structurally correct but noise for review.
                let total_lines: u32 = overlaps
                    .iter()
                    .map(|r| r.end.saturating_sub(r.start) + 1)
                    .sum();
                if ignore_test_trivia
                    && is_test_symbol(&sym.path, Some(sym.qualified_name.as_str()))
                    && total_lines <= 2
                {
                    continue;
                }
                summary.symbols_modified_body += 1;
                let ranges = overlaps
                    .iter()
                    .map(|r| format!("{}-{}", r.start, r.end))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(PendingComment {
                    comment: ReviewComment {
                        file: sym.path.clone(),
                        line: sym.anchor.start_line,
                        end_line: sym.anchor.end_line,
                        symbol_qualified_name: Some(sym.qualified_name.clone()),
                        symbol_id: Some(sym.id.0.clone()),
                        source_kind: "modified-body-touched".to_string(),
                        confidence_tier: "inferred".to_string(),
                        rule_id: None,
                        body: format!(
                            "`{}` was touched by the PR diff (lines {}) without changing its \
                            outbound call set or byte-span. Single-line guard / assertion / \
                            inline tweak — verify the change reads correctly in context.",
                            sym.qualified_name, ranges
                        ),
                    },
                    symbol_kind: Some(sym.kind),
                    anchor: Some(anchor),
                });
            }
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

#[cfg(test)]
mod precision_tests {
    use super::*;

    fn pc(file: &str, span: (u64, u64), kind: Option<SymbolKind>, src_kind: &str) -> PendingComment {
        PendingComment {
            comment: ReviewComment {
                file: file.to_string(),
                line: 0,
                end_line: 0,
                symbol_qualified_name: None,
                symbol_id: None,
                source_kind: src_kind.to_string(),
                confidence_tier: "inferred".to_string(),
                rule_id: None,
                body: String::new(),
            },
            symbol_kind: kind,
            anchor: Some(span),
        }
    }

    #[test]
    fn pattern_a_drops_container_when_leaf_present() {
        // Class anchor 0..1000 with a method anchor 100..500 inside.
        // The method's comment is the high-signal observation; the
        // class's "body length changed" restates the diff.
        let mut p = vec![
            pc("a.rs", (0, 1000), Some(SymbolKind::Struct), "modified-body-logic"),
            pc("a.rs", (100, 500), Some(SymbolKind::Method), "modified-body"),
        ];
        suppress_container_restate(&mut p);
        assert_eq!(p.len(), 1, "container dropped; leaf kept");
        assert_eq!(p[0].symbol_kind, Some(SymbolKind::Method));
    }

    #[test]
    fn pattern_a_keeps_container_when_no_leaf_inside() {
        // Class with span change but no enclosed method emits.
        let mut p = vec![pc("a.rs", (0, 1000), Some(SymbolKind::Struct), "modified-body-logic")];
        suppress_container_restate(&mut p);
        assert_eq!(p.len(), 1, "lone container survives — no leaf to defer to");
    }

    #[test]
    fn pattern_a_does_not_drop_across_files() {
        let mut p = vec![
            pc("a.rs", (0, 1000), Some(SymbolKind::Struct), "modified-body-logic"),
            pc("b.rs", (100, 500), Some(SymbolKind::Method), "modified-body"),
        ];
        suppress_container_restate(&mut p);
        assert_eq!(p.len(), 2, "different files — no suppression");
    }

    #[test]
    fn pattern_d_callable_predicate() {
        assert!(is_callable_kind(SymbolKind::Function));
        assert!(is_callable_kind(SymbolKind::Method));
        assert!(is_callable_kind(SymbolKind::Macro));
        assert!(!is_callable_kind(SymbolKind::Struct));
        assert!(!is_callable_kind(SymbolKind::Trait));
        assert!(!is_callable_kind(SymbolKind::TypeAlias));
        assert!(!is_callable_kind(SymbolKind::Const));
        assert!(!is_callable_kind(SymbolKind::Static));
        assert!(!is_callable_kind(SymbolKind::Module));
    }

    #[test]
    fn pattern_a2_drops_outer_fn_when_inner_fn_present() {
        // axios shape: outer named fn-expr (httpAdapter) wraps inner
        // named fn (dispatchHttpRequest). Both emit modified-body-logic;
        // the outer's comment is restate-of-diff, drop it.
        let mut p = vec![
            pc("a.js", (1000, 5000), Some(SymbolKind::Function), "modified-body-logic"),
            pc("a.js", (1500, 4500), Some(SymbolKind::Function), "modified-body"),
        ];
        suppress_outer_function_restate(&mut p);
        assert_eq!(p.len(), 1, "outer fn dropped; inner fn kept");
        // Inner survives — anchor (1500, 4500).
        assert_eq!(p[0].anchor, Some((1500, 4500)));
    }

    #[test]
    fn pattern_a2_does_not_drop_sibling_functions() {
        // Two non-overlapping fns in the same file — both kept.
        let mut p = vec![
            pc("a.js", (1000, 2000), Some(SymbolKind::Function), "modified-body"),
            pc("a.js", (3000, 4000), Some(SymbolKind::Function), "modified-body"),
        ];
        suppress_outer_function_restate(&mut p);
        assert_eq!(p.len(), 2, "sibling fns both kept");
    }

    #[test]
    fn pattern_a2_method_inside_method_drops_outer() {
        // Method enclosing Method — also covered.
        let mut p = vec![
            pc("a.ts", (100, 500), Some(SymbolKind::Method), "modified-body"),
            pc("a.ts", (200, 300), Some(SymbolKind::Method), "modified-body"),
        ];
        suppress_outer_function_restate(&mut p);
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn pattern_c_test_path_classifier() {
        assert!(is_test_path("tests/foo.rs"));
        assert!(is_test_path("tests/regression.rs"));
        assert!(is_test_path("crates/searcher/tests/integration.rs"));
        assert!(is_test_path("test/foo.py"));
        assert!(is_test_path("src/__tests__/widget.tsx"));
        assert!(is_test_path("lib/foo_test.rs"));
        assert!(is_test_path("lib/components/Button.test.tsx"));
        assert!(is_test_path("lib/components/Button.spec.ts"));
        assert!(is_test_path("python/test_widget.py"));
        assert!(is_test_path("python/widget_test.py"));
        assert!(!is_test_path("src/foo.rs"));
        assert!(!is_test_path("lib/widget.ts"));
        assert!(!is_test_path("src/test_helpers.rs"));
    }

    #[test]
    fn pattern_a_container_kinds() {
        for k in [
            SymbolKind::Struct,
            SymbolKind::Trait,
            SymbolKind::TypeAlias,
            SymbolKind::Module,
            SymbolKind::Enum,
        ] {
            assert!(is_container_kind(Some(k)), "{:?} should be container", k);
        }
        for k in [
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Macro,
            SymbolKind::Const,
            SymbolKind::Static,
        ] {
            assert!(!is_container_kind(Some(k)), "{:?} should NOT be container", k);
        }
    }
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
        diff_hunks: None,
        ignore_test_trivia: false,
    })
}
