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
    })
    .unwrap();
    assert_eq!(result.comments.len(), 1, "budget should cap at 1");
    assert!(
        result.summary.comments_dropped_to_budget >= 1,
        "summary should record dropped count"
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
    })
    .unwrap();
    let md = pr_review::render_markdown(&result);
    assert!(md.contains("# mallard PR review"));
    assert!(md.contains("[structural-rule]"), "badge prefix present in markdown");
}
