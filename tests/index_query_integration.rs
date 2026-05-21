use std::path::PathBuf;

use duckdb::Connection;
use mallard::{
    BuildRequest, Direction, EdgeKind, FindingFilter, IndexReader, MallardError, QueryRequest,
    QueryResult, SymbolId, build,
};
use tempfile::TempDir;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample-rust")
}

fn fixture_rules() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("rules.yml")
}

fn build_fixture(out: &PathBuf, with_rules: bool) {
    let req = BuildRequest {
        root: fixture_root(),
        sha: "deadbeefcafe".to_string(),
        rules_path: if with_rules { Some(fixture_rules()) } else { None },
        out_path: out.clone(),
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["rust".to_string()],
        slowest_files_n: 10,
    };
    build(req).unwrap();
}

fn open_reader(index: &PathBuf) -> IndexReader {
    IndexReader::open(index).unwrap()
}

fn find_symbol(index: &PathBuf, path: &str, qualified_name: &str) -> SymbolId {
    let symbols = open_reader(index).symbols_in_file(path).unwrap();
    symbols
        .into_iter()
        .find(|s| s.qualified_name == qualified_name)
        .unwrap_or_else(|| panic!("no symbol {qualified_name} in {path}"))
        .id
}

#[test]
fn metadata_returns_sha_and_format_version() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, true);

    let meta = open_reader(&out).metadata().unwrap();
    assert_eq!(meta.sha.as_deref(), Some("deadbeefcafe"));
    assert_eq!(meta.index_format_version, 1);
    assert!(meta.indexer_version.is_some());
    assert!(meta.rule_set_hash.is_some());
    assert_eq!(meta.language_allow_list, vec!["rust".to_string()]);
}

#[test]
fn lookup_symbol_present_and_missing() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let id = find_symbol(&out, "lib.rs", "double");
    let found = open_reader(&out)
        .lookup_symbol(&id)
        .unwrap()
        .expect("symbol present");
    assert_eq!(found.qualified_name, "double");
    assert_eq!(found.path, "lib.rs");

    let missing = open_reader(&out)
        .lookup_symbol(&SymbolId("0".repeat(32)))
        .unwrap();
    assert!(missing.is_none());
}

#[test]
fn symbols_in_file_enriches_path_and_anchor() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let syms = open_reader(&out).symbols_in_file("greet.rs").unwrap();
    let greet = syms
        .iter()
        .find(|s| s.qualified_name == "greet")
        .expect("greet symbol present");
    assert_eq!(greet.path, "greet.rs");
    assert!(greet.anchor.end_line >= greet.anchor.start_line);
}

#[test]
fn neighbors_out_calls_from_bump_reach_double() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let bump = find_symbol(&out, "lib.rs", "Counter::bump");
    let edges = open_reader(&out)
        .neighbors(&bump, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    assert!(!edges.is_empty(), "bump should call something");
    let mentions_double = edges.iter().any(|e| {
        e.dst.as_ref().map(|d| d.qualified_name == "double").unwrap_or(false)
            || e.dst_unresolved.as_deref() == Some("double")
    });
    assert!(mentions_double, "expected a calls edge naming double, got {edges:?}");
}

#[test]
fn expand_depth_zero_returns_source_only() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let id = find_symbol(&out, "lib.rs", "double");
    let g = open_reader(&out)
        .expand(&id, 0, &[], Direction::Both)
        .unwrap();
    assert_eq!(g.nodes.len(), 1);
    assert!(g.edges.is_empty());
    assert_eq!(g.max_depth_reached, 0);
}

#[test]
fn expand_depth_two_reaches_callees_transitively() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let bump = find_symbol(&out, "lib.rs", "Counter::bump");
    let g = open_reader(&out)
        .expand(&bump, 2, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    assert!(g.nodes.len() >= 2);
    assert!(g.edges.iter().any(|e| e.kind == EdgeKind::Calls));
    assert!(g.max_depth_reached >= 1);
}

#[test]
fn findings_filter_by_rule_id() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, true);

    let all = open_reader(&out)
        .findings(FindingFilter::default())
        .unwrap();
    assert!(!all.is_empty());

    let format_only = open_reader(&out)
        .findings(FindingFilter {
            rule_id: Some("rust-format-macro".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert!(format_only.iter().all(|f| f.rule_id == "rust-format-macro"));
    assert!(!format_only.is_empty());
}

#[test]
fn findings_filter_by_path_prefix() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, true);

    let only_main = open_reader(&out)
        .findings(FindingFilter {
            path_prefix: Some("main.rs".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert!(!only_main.is_empty());
    assert!(only_main.iter().all(|f| f.path.starts_with("main.rs")));
}

#[test]
fn findings_filter_by_symbol_id_limits_to_anchor_lines() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, true);

    let greet = find_symbol(&out, "greet.rs", "greet");
    let scoped = open_reader(&out)
        .findings(FindingFilter {
            symbol_id: Some(greet.clone()),
            ..Default::default()
        })
        .unwrap();
    assert!(!scoped.is_empty(), "expected format! finding inside greet");
    assert!(scoped.iter().all(|f| f.path == "greet.rs"));
}

#[test]
fn files_at_prefix_returns_matching_files() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let all = open_reader(&out).files_at_prefix("").unwrap();
    assert!(all.len() >= 4);
    let only_lib = open_reader(&out).files_at_prefix("lib.rs").unwrap();
    assert_eq!(only_lib.len(), 1);
    assert_eq!(only_lib[0].path, "lib.rs");

    let none = open_reader(&out).files_at_prefix("no/such/").unwrap();
    assert!(none.is_empty());
}

#[test]
fn missing_index_file_errors() {
    let bogus = PathBuf::from("./does-not-exist.duckdb");
    let err = IndexReader::open(&bogus).err().expect("should error");
    assert!(matches!(err, MallardError::IndexNotFound(_)), "got: {err}");
}

#[test]
fn run_dispatches_query_request_metadata() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let json = r#"{"kind":"metadata"}"#;
    let request: QueryRequest = serde_json::from_str(json).unwrap();
    let result = open_reader(&out).run(&request).unwrap();
    match result {
        QueryResult::Metadata(m) => assert_eq!(m.sha.as_deref(), Some("deadbeefcafe")),
        other => panic!("expected Metadata result, got {other:?}"),
    }
}

#[test]
fn run_dispatches_lookup_and_expand() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let bump = find_symbol(&out, "lib.rs", "Counter::bump");
    let reader = open_reader(&out);

    let lookup = reader
        .run(&QueryRequest::LookupSymbol { id: bump.clone() })
        .unwrap();
    match lookup {
        QueryResult::LookupSymbol(Some(sym)) => assert_eq!(sym.qualified_name, "Counter::bump"),
        other => panic!("expected Some symbol, got {other:?}"),
    }

    let expand = reader
        .run(&QueryRequest::Expand {
            id: bump,
            depth: 1,
            kinds: vec![EdgeKind::Calls],
            direction: Direction::Out,
        })
        .unwrap();
    match expand {
        QueryResult::Expand(g) => assert!(g.nodes.len() >= 2),
        other => panic!("expected Expand result, got {other:?}"),
    }
}

#[test]
fn version_mismatch_is_explicit() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    {
        let conn = Connection::open(&out).unwrap();
        conn.execute(
            "UPDATE metadata SET value='99' WHERE key='index_format_version'",
            [],
        )
        .unwrap();
        conn.close().unwrap();
    }

    let err = IndexReader::open(&out).err().expect("should error");
    match err {
        MallardError::VersionMismatch { found, expected } => {
            assert_eq!(found, 99);
            assert_eq!(expected, 1);
        }
        other => panic!("expected VersionMismatch, got {other:?}"),
    }
}
