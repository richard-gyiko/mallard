use std::path::PathBuf;

use duckdb::Connection;
use mallard::schema::{cols, metadata_keys, tables};
use mallard::{BuildRequest, build};
use tempfile::TempDir;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample-rust")
}

fn python_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample-python")
}

fn fixture_rules() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("rules.yml")
}

fn make_request(root: PathBuf, sha: &str, out: PathBuf) -> BuildRequest {
    BuildRequest {
        root,
        sha: sha.to_string(),
        rules_path: None,
        out_path: out,
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["rust".to_string()],
        slowest_files_n: 10,
    }
}

fn make_request_with_rules(
    root: PathBuf,
    sha: &str,
    out: PathBuf,
    rules: PathBuf,
) -> BuildRequest {
    BuildRequest {
        rules_path: Some(rules),
        ..make_request(root, sha, out)
    }
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn count_where(conn: &Connection, table: &str, column: &str, value: &str) -> i64 {
    conn.query_row(
        &format!("SELECT count(*) FROM {table} WHERE {column} = ?"),
        [value],
        |r| r.get(0),
    )
    .unwrap()
}

fn metadata_value(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        &format!(
            "SELECT {} FROM {} WHERE {} = ?",
            cols::metadata::VALUE,
            tables::METADATA,
            cols::metadata::KEY,
        ),
        [key],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

#[test]
fn happy_path_indexes_sample_repo() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    let summary = build(make_request(fixture_root(), "deadbeefcafe", out.clone())).unwrap();

    assert_eq!(summary.sha, "deadbeefcafe");
    assert!(out.exists(), "index file written");
    assert!(summary.counters.symbols > 0);

    let conn = Connection::open(&out).unwrap();

    let files = count(&conn, tables::FILES);
    assert!(files >= 4, "expected at least 4 files indexed, got {files}");

    let functions = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "function");
    assert!(
        functions >= 3,
        "expected at least 3 function symbols (double, greet, main), got {functions}"
    );

    let methods = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "method");
    assert!(
        methods >= 2,
        "expected at least 2 methods on Counter (new, bump), got {methods}"
    );

    let structs = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "struct");
    assert!(structs >= 1, "expected at least 1 struct (Counter), got {structs}");

    let contains = count_where(&conn, tables::EDGES, cols::edges::KIND, "contains");
    assert!(contains > 0, "expected contains edges, got {contains}");

    let calls = count_where(&conn, tables::EDGES, cols::edges::KIND, "calls");
    assert!(calls > 0, "expected calls edges, got {calls}");

    let imports = count_where(&conn, tables::EDGES, cols::edges::KIND, "imports");
    assert!(imports > 0, "expected imports edges, got {imports}");

    assert_eq!(
        metadata_value(&conn, metadata_keys::SHA).as_deref(),
        Some("deadbeefcafe"),
    );
    assert!(
        metadata_value(&conn, metadata_keys::INDEX_FORMAT_VERSION).is_some(),
        "index_format_version stamped in metadata"
    );
}

#[test]
fn python_files_index_without_crashing() {
    // A1 scaffolding test: `.py` files dispatched to PythonExtractor.
    // No symbol extraction yet (A2 land); the contract is "doesn't crash
    // and the file is recorded as Indexed."
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("python.duckdb");
    let req = BuildRequest {
        root: python_fixture_root(),
        sha: "py-scaffold".to_string(),
        rules_path: None,
        out_path: out.clone(),
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["python".to_string()],
        slowest_files_n: 10,
    };
    let summary = build(req).unwrap();
    assert_eq!(summary.sha, "py-scaffold");
    assert!(out.exists(), "python index file written");

    let conn = Connection::open(&out).unwrap();
    let py_files = count_where(&conn, tables::FILES, cols::files::LANGUAGE, "python");
    assert!(
        py_files >= 2,
        "expected at least 2 python files indexed, got {py_files}"
    );
}

#[test]
fn rules_produce_findings_and_metadata_hash() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("rules.duckdb");
    let summary = build(make_request_with_rules(
        fixture_root(),
        "deadbeefcafe",
        out.clone(),
        fixture_rules(),
    ))
    .unwrap();

    assert!(
        summary.counters.findings >= 2,
        "expected at least 2 findings (format! in greet.rs, println! in main.rs), got {}",
        summary.counters.findings
    );
    assert!(summary.rule_set_hash.is_some(), "rule_set_hash stamped");

    let conn = Connection::open(&out).unwrap();
    let total = count(&conn, tables::FINDINGS);
    assert_eq!(
        total as u64,
        summary.counters.findings,
        "counters.findings matches findings table row count",
    );

    let format_hits =
        count_where(&conn, tables::FINDINGS, cols::findings::RULE_ID, "rust-format-macro");
    assert!(
        format_hits >= 1,
        "expected at least 1 hit for rust-format-macro, got {format_hits}"
    );
    let println_hits =
        count_where(&conn, tables::FINDINGS, cols::findings::RULE_ID, "rust-println-macro");
    assert!(
        println_hits >= 1,
        "expected at least 1 hit for rust-println-macro, got {println_hits}"
    );

    assert_eq!(
        metadata_value(&conn, metadata_keys::RULE_SET_HASH),
        summary.rule_set_hash,
        "rule_set_hash in metadata table matches summary",
    );
}

#[test]
fn no_rules_produces_zero_findings() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("no-rules.duckdb");
    let summary = build(make_request(fixture_root(), "deadbeefcafe", out.clone())).unwrap();
    assert_eq!(summary.counters.findings, 0);
    let conn = Connection::open(&out).unwrap();
    assert_eq!(count(&conn, tables::FINDINGS), 0);
    assert!(summary.rule_set_hash.is_none());
}

#[test]
fn rebuild_is_deterministic() {
    let tmp = TempDir::new().unwrap();
    let out_a = tmp.path().join("a.duckdb");
    let out_b = tmp.path().join("b.duckdb");

    build(make_request(fixture_root(), "deadbeefcafe", out_a.clone())).unwrap();
    build(make_request(fixture_root(), "deadbeefcafe", out_b.clone())).unwrap();

    let ids_a = symbol_ids(&out_a);
    let ids_b = symbol_ids(&out_b);
    assert_eq!(ids_a, ids_b, "symbol IDs should be identical across rebuilds");

    let edges_a = edge_tuples(&out_a);
    let edges_b = edge_tuples(&out_b);
    assert_eq!(edges_a, edges_b, "edge content should be identical across rebuilds");
}

#[test]
fn empty_repo_produces_valid_index() {
    let repo = TempDir::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("empty.duckdb");
    let summary = build(make_request(
        repo.path().to_path_buf(),
        "0000000000",
        out.clone(),
    ))
    .unwrap();

    assert_eq!(summary.counters.symbols, 0);
    let conn = Connection::open(&out).unwrap();
    assert_eq!(count(&conn, tables::FILES), 0);
    assert_eq!(
        metadata_value(&conn, metadata_keys::SHA).as_deref(),
        Some("0000000000"),
    );
}

#[test]
fn parse_failure_is_recorded_and_other_files_continue() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("with-broken.duckdb");
    build(make_request(fixture_root(), "deadbeefcafe", out.clone())).unwrap();
    let conn = Connection::open(&out).unwrap();

    let parse_err_count = count(&conn, tables::PARSE_ERRORS);
    assert!(
        parse_err_count > 0,
        "expected parse_errors row for broken.rs, got {parse_err_count}"
    );

    let other_symbols: i64 = conn
        .query_row(
            &format!(
                "SELECT count(*) FROM {s} JOIN {f} ON {f}.{fid} = {s}.{sfid} WHERE {f}.{p} != 'broken.rs'",
                s = tables::SYMBOLS,
                f = tables::FILES,
                fid = cols::files::FILE_ID,
                sfid = cols::symbols::FILE_ID,
                p = cols::files::PATH,
            ),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        other_symbols > 0,
        "expected symbols from non-broken files even when one file fails to parse"
    );
}

#[test]
fn skip_by_size_marker_is_recorded() {
    let repo = TempDir::new().unwrap();
    let big = repo.path().join("big.rs");
    std::fs::write(&big, vec![b'a'; 2 * 1024 * 1024]).unwrap();
    std::fs::write(repo.path().join("small.rs"), "pub fn tiny() {}\n").unwrap();

    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("size.duckdb");
    build(make_request(
        repo.path().to_path_buf(),
        "feedface",
        out.clone(),
    ))
    .unwrap();

    let conn = Connection::open(&out).unwrap();
    let skipped = count_where(&conn, tables::FILES, cols::files::STATUS, "skipped:size");
    assert_eq!(skipped, 1, "expected one size-skipped file");

    let tiny_symbols: i64 = conn
        .query_row(
            &format!(
                "SELECT count(*) FROM {s} JOIN {f} ON {f}.{fid} = {s}.{sfid} WHERE {f}.{p} = 'small.rs'",
                s = tables::SYMBOLS,
                f = tables::FILES,
                fid = cols::files::FILE_ID,
                sfid = cols::symbols::FILE_ID,
                p = cols::files::PATH,
            ),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tiny_symbols, 1, "small.rs should still produce its symbol");
}

fn symbol_ids(path: &PathBuf) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {sid} FROM {s} ORDER BY {sid}",
            sid = cols::symbols::SYMBOL_ID,
            s = tables::SYMBOLS,
        ))
        .unwrap();
    let rows: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows
}

fn edge_tuples(path: &PathBuf) -> Vec<(String, Option<String>, Option<String>, String, i64)> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {src}, {dst}, {unr}, {kind}, {fid} FROM {e} ORDER BY {kind}, {src}, {dst}, {unr}",
            src = cols::edges::SRC_SYMBOL_ID,
            dst = cols::edges::DST_SYMBOL_ID,
            unr = cols::edges::DST_UNRESOLVED,
            kind = cols::edges::KIND,
            fid = cols::edges::FILE_ID,
            e = tables::EDGES,
        ))
        .unwrap();
    let rows: Vec<(String, Option<String>, Option<String>, String, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows
}
