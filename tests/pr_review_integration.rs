//! Integration test for the deterministic-only PR review pipeline.
//! Validates that a build → pr-review chain produces a non-trivial
//! comments list when base + head differ on the same fixture set.

use std::path::PathBuf;

use mallard::pr_review::{self, PrReviewRequest};
use mallard::{BuildRequest, build};
use tempfile::TempDir;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample-rust")
}

fn build_at(sha: &str, out: PathBuf) {
    let req = BuildRequest {
        root: fixture_root(),
        sha: sha.to_string(),
        rules_path: Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("rules.yml"),
        ),
        rules_bundled: false,
        out_path: out,
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["rust".to_string()],
        slowest_files_n: 10,
    };
    build(req).unwrap();
}

#[test]
fn pr_review_emits_structural_rule_comments_on_changed_files() {
    // Base and head point at the same source tree (no real diff) but the
    // changed-files list still drives the per-file finding scan. With the
    // bundled rules.yml the fixture surfaces `rust-format-macro` +
    // `rust-println-macro` hits — those propagate as structural-rule
    // comments regardless of the base/head diff.
    let tmp = TempDir::new().unwrap();
    let base_db = tmp.path().join("base.duckdb");
    let head_db = tmp.path().join("head.duckdb");
    build_at("base-sha", base_db.clone());
    build_at("head-sha", head_db.clone());

    let result = pr_review::run(PrReviewRequest {
        base_db,
        head_db,
        changed_files: vec!["main.rs".to_string(), "greet.rs".to_string()],
        max_comments: 20,
        diff_hunks: None,
        ignore_test_trivia: false,
    })
    .unwrap();

    assert!(
        result.summary.comments_emitted >= 2,
        "expected ≥2 structural-rule comments, got {}",
        result.summary.comments_emitted
    );
    let any_structural = result
        .comments
        .iter()
        .any(|c| c.confidence_tier == "structural-rule" && c.rule_id.is_some());
    assert!(any_structural, "expected at least one structural-rule comment");
}

#[test]
fn pr_review_respects_max_comments_budget() {
    let tmp = TempDir::new().unwrap();
    let base_db = tmp.path().join("base.duckdb");
    let head_db = tmp.path().join("head.duckdb");
    build_at("base-sha", base_db.clone());
    build_at("head-sha", head_db.clone());

    let result = pr_review::run(PrReviewRequest {
        base_db,
        head_db,
        changed_files: vec!["main.rs".to_string(), "greet.rs".to_string(), "lib.rs".to_string()],
        max_comments: 1,
        diff_hunks: None,
        ignore_test_trivia: false,
    })
    .unwrap();
    assert_eq!(result.comments.len(), 1, "budget should cap at 1");
    assert!(
        result.summary.comments_dropped_to_budget >= 1,
        "summary should record dropped count"
    );
}

#[test]
fn parse_unified_diff_hunks_extracts_per_file_ranges() {
    let diff = "\
diff --git a/src/foo.rs b/src/foo.rs\n\
--- a/src/foo.rs\n\
+++ b/src/foo.rs\n\
@@ -10,2 +10,3 @@ fn enclosing()\n\
-old line a\n\
-old line b\n\
+new line a\n\
+new line b\n\
+new line c\n\
@@ -42 +43 @@ fn other()\n\
-old\n\
+new\n\
@@ -100,2 +100,0 @@ fn deleted()\n\
-bye 1\n\
-bye 2\n\
diff --git a/src/bar.py b/src/bar.py\n\
--- a/src/bar.py\n\
+++ b/src/bar.py\n\
@@ -5,1 +5,1 @@ def thing()\n\
-x\n\
+y\n\
";
    let hunks = mallard::pr_review::parse_unified_diff_hunks(diff);
    let foo = hunks.files.get("src/foo.rs").expect("src/foo.rs hunks present");
    assert_eq!(foo.len(), 2, "deletion-only hunk skipped, two head-side hunks remain");
    assert_eq!(foo[0].start, 10);
    assert_eq!(foo[0].end, 12);
    assert_eq!(foo[1].start, 43);
    assert_eq!(foo[1].end, 43);
    let bar = hunks.files.get("src/bar.py").expect("src/bar.py hunks present");
    assert_eq!(bar.len(), 1);
    assert_eq!(bar[0].start, 5);
    assert_eq!(bar[0].end, 5);
}

#[test]
fn diff_hunks_overlap_emits_modified_body_touched() {
    use std::collections::HashMap;

    let tmp = TempDir::new().unwrap();
    let base_db = tmp.path().join("base.duckdb");
    let head_db = tmp.path().join("head.duckdb");
    build_at("base-sha", base_db.clone());
    build_at("head-sha", head_db.clone());

    let bump_line = {
        let reader = mallard::IndexReader::open(&head_db).unwrap();
        let syms = reader.symbols_in_file("lib.rs").unwrap();
        let bump = syms.iter().find(|s| s.qualified_name == "Counter::bump").unwrap();
        bump.anchor.start_line + 1
    };

    let mut files = HashMap::new();
    files.insert(
        "lib.rs".to_string(),
        vec![mallard::pr_review::DiffRange {
            start: bump_line,
            end: bump_line,
        }],
    );

    let result = mallard::pr_review::run(mallard::pr_review::PrReviewRequest {
        base_db,
        head_db,
        changed_files: vec!["lib.rs".to_string()],
        max_comments: 20,
        diff_hunks: Some(mallard::pr_review::DiffHunks { files }),
        ignore_test_trivia: false,
    })
    .unwrap();

    let touched = result
        .comments
        .iter()
        .find(|c| c.source_kind == "modified-body-touched")
        .expect("Finding-7 `modified-body-touched` comment present");
    assert_eq!(touched.confidence_tier, "inferred");
    assert!(touched.body.contains("Counter::bump"));
}

#[test]
fn pattern_b_structural_rule_gated_by_diff_hunk_overlap() {
    use std::collections::HashMap;

    // Build base+head with the rules fixture pointing at rust-format-macro
    // hits in greet.rs. Without diff hunks, the rule fires (backward
    // compat). With diff hunks that DON'T overlap the finding line, the
    // rule is suppressed — Pattern B from the M4 grading writeup.
    let tmp = TempDir::new().unwrap();
    let base_db = tmp.path().join("base.duckdb");
    let head_db = tmp.path().join("head.duckdb");
    build_at("base-sha", base_db.clone());
    build_at("head-sha", head_db.clone());

    // Baseline: no diff_hunks → rule fires (request lacks `--diff-hunks`).
    let baseline = mallard::pr_review::run(mallard::pr_review::PrReviewRequest {
        base_db: base_db.clone(),
        head_db: head_db.clone(),
        changed_files: vec!["greet.rs".to_string()],
        max_comments: 20,
        diff_hunks: None,
        ignore_test_trivia: false,
    })
    .unwrap();
    let baseline_rule_hits = baseline
        .comments
        .iter()
        .filter(|c| c.source_kind == "structural-rule")
        .count();
    assert!(
        baseline_rule_hits >= 1,
        "baseline (no diff_hunks) should emit ≥1 structural-rule comment, got {baseline_rule_hits}"
    );

    // With diff_hunks but a range that's NOT where the finding lives
    // (lines 999-1000), suppress the rule emission.
    let mut files = HashMap::new();
    files.insert(
        "greet.rs".to_string(),
        vec![mallard::pr_review::DiffRange { start: 999, end: 1000 }],
    );
    let gated = mallard::pr_review::run(mallard::pr_review::PrReviewRequest {
        base_db,
        head_db,
        changed_files: vec!["greet.rs".to_string()],
        max_comments: 20,
        diff_hunks: Some(mallard::pr_review::DiffHunks { files }),
        ignore_test_trivia: false,
    })
    .unwrap();
    let gated_rule_hits = gated
        .comments
        .iter()
        .filter(|c| c.source_kind == "structural-rule")
        .count();
    assert_eq!(
        gated_rule_hits, 0,
        "rule outside the diff hunks should be suppressed; got {gated_rule_hits}"
    );
}

#[test]
fn pr_review_markdown_render_includes_badge() {
    let tmp = TempDir::new().unwrap();
    let base_db = tmp.path().join("base.duckdb");
    let head_db = tmp.path().join("head.duckdb");
    build_at("base-sha", base_db.clone());
    build_at("head-sha", head_db.clone());

    let result = pr_review::run(PrReviewRequest {
        base_db,
        head_db,
        changed_files: vec!["greet.rs".to_string()],
        max_comments: 5,
        diff_hunks: None,
        ignore_test_trivia: false,
    })
    .unwrap();
    let md = pr_review::render_markdown(&result);
    assert!(md.contains("# mallard PR review"));
    assert!(md.contains("[structural-rule]"), "badge prefix present in markdown");
}
