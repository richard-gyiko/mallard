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

fn typescript_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample-typescript")
}

fn typescript_request(out: PathBuf) -> BuildRequest {
    BuildRequest {
        root: typescript_fixture_root(),
        sha: "ts-fixture".to_string(),
        rules_path: None,
        rules_bundled: false,
        out_path: out,
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["typescript".to_string(), "tsx".to_string()],
        slowest_files_n: 10,
    }
}

fn javascript_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample-javascript")
}

fn javascript_request(out: PathBuf) -> BuildRequest {
    BuildRequest {
        root: javascript_fixture_root(),
        sha: "js-fixture".to_string(),
        rules_path: None,
        rules_bundled: false,
        out_path: out,
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["typescript".to_string(), "tsx".to_string()],
        slowest_files_n: 10,
    }
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
        rules_bundled: false,
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
        rules_bundled: false,
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

fn python_request(out: PathBuf) -> BuildRequest {
    BuildRequest {
        root: python_fixture_root(),
        sha: "py-fixture".to_string(),
        rules_path: None,
        rules_bundled: false,
        out_path: out,
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["python".to_string()],
        slowest_files_n: 10,
    }
}

#[test]
fn bundled_rules_fire_on_security_grade_python_patterns() {
    // Pilot Finding 5 + Finding 6: structural-rule findings should fire
    // out of the box. The bundled rule pack ships with the binary and
    // loads via the new `rules_bundled` BuildRequest field. After the
    // Finding-6 tightening, only security/correctness-grade rules stay
    // bundled — verify against `risky_eval` / `risky_exec` in
    // `sample-python/app.py` (python-eval-use + python-exec-use).
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("bundled.duckdb");
    let req = BuildRequest {
        rules_path: None,
        rules_bundled: true,
        ..python_request(out.clone())
    };
    let summary = build(req).unwrap();
    assert!(
        summary.counters.findings >= 2,
        "bundled pack should hit eval + exec in sample-python, got {}",
        summary.counters.findings
    );
    assert!(
        summary.rule_set_hash.as_ref().is_some_and(|h| h.starts_with("bundled:")),
        "rule_set_hash should carry `bundled:` prefix, got {:?}",
        summary.rule_set_hash
    );

    // Style rule dropped in Finding-6 tightening must NOT fire in the
    // bundled pack. Users who want stricter style policing add it via
    // their own --rules path.
    let conn = Connection::open(&out).unwrap();
    let print_hits = count_where(
        &conn,
        tables::FINDINGS,
        cols::findings::RULE_ID,
        "python-print-call",
    );
    assert_eq!(
        print_hits, 0,
        "python-print-call should not be in the bundled pack (Finding 6)"
    );
}

#[test]
fn javascript_arrow_and_function_expressions_index_as_function_symbols() {
    // Finding 8: anonymous-but-named function expressions and
    // const-bound arrow functions must index as Function symbols. axios
    // PR #10901's `export default ... && function httpAdapter(c) {}` shape
    // was invisible to mallard before this fix; pilot v3 left it at 0
    // comments. After the fix, the same shape in `widget.jsx` becomes a
    // first-class Function symbol.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("javascript-fn-expr.duckdb");
    build(javascript_request(out.clone())).unwrap();
    let conn = Connection::open(&out).unwrap();
    let arrow_hit = conn
        .query_row(
            "SELECT count(*) FROM symbols WHERE qualified_name = ?",
            ["adornArrow"],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert!(arrow_hit >= 1, "arrow function `adornArrow` should index");
    let named_fn_expr_hit = conn
        .query_row(
            "SELECT count(*) FROM symbols WHERE qualified_name = ?",
            ["namedFnExpr"],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert!(
        named_fn_expr_hit >= 1,
        "named function_expression `namedFnExpr` (axios #10901 shape) should index"
    );
}

#[test]
fn javascript_files_index_via_typescript_grammar() {
    // Pilot Finding 3: `.js` / `.mjs` / `.cjs` / `.jsx` dispatched through
    // the TS / TSX grammar so JavaScript-shop repos (axios, lodash, …)
    // produce structural evidence instead of being silently skipped.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("javascript.duckdb");
    let summary = build(javascript_request(out.clone())).unwrap();
    assert!(out.exists(), "js index file written");
    assert!(
        summary.counters.symbols >= 5,
        "expected ≥5 symbols (double, Counter, constructor, bump, main, Widget, formatLabel), got {}",
        summary.counters.symbols
    );

    let conn = Connection::open(&out).unwrap();
    // Three different extensions land — verify all three were indexed.
    let total_files = count(&conn, tables::FILES);
    assert!(total_files >= 3, "expected ≥3 indexed JS-family files, got {total_files}");
    let calls = count_where(&conn, tables::EDGES, cols::edges::KIND, "calls");
    assert!(calls >= 2, "expected ≥2 calls edges, got {calls}");
    let imports = count_where(&conn, tables::EDGES, cols::edges::KIND, "imports");
    assert!(imports >= 1, "expected ≥1 imports edge (app.mjs → lib.js), got {imports}");
}

#[test]
fn typescript_index_extracts_symbols_and_edges() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("typescript.duckdb");
    let summary = build(typescript_request(out.clone())).unwrap();
    assert!(out.exists(), "ts index file written");
    assert!(
        summary.counters.symbols >= 6,
        "expected ≥6 symbols (double, Counter, bump, Named, CounterFactory, main, Inner+ping, Outer+ping+echo, ...), got {}",
        summary.counters.symbols
    );

    let conn = Connection::open(&out).unwrap();
    let functions = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "function");
    assert!(functions >= 2, "expected ≥2 functions (double, main, bareSolo…), got {functions}");
    let methods = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "method");
    assert!(
        methods >= 3,
        "expected ≥3 methods (Counter.bump, Inner.ping, Outer.ping, Outer.echo, …), got {methods}"
    );
    let classes = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "struct");
    assert!(classes >= 3, "expected ≥3 classes (Counter, Inner, Outer, OnlyMethod), got {classes}");
    let interfaces = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "trait");
    assert!(interfaces >= 1, "expected ≥1 interface (Named), got {interfaces}");

    let imports = count_where(&conn, tables::EDGES, cols::edges::KIND, "imports");
    assert!(imports >= 1, "expected ≥1 imports edge (app.ts imports lib), got {imports}");
    let calls = count_where(&conn, tables::EDGES, cols::edges::KIND, "calls");
    assert!(calls >= 2, "expected ≥2 calls (double, bump, ping, …), got {calls}");
}

#[test]
fn python_rules_produce_findings() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("python-rules.duckdb");
    let req = BuildRequest {
        rules_path: Some(fixture_rules()),
        rules_bundled: false,
        ..python_request(out.clone())
    };
    let summary = build(req).unwrap();
    assert!(
        summary.counters.findings >= 3,
        "expected ≥3 python findings (eval, exec, print), got {}",
        summary.counters.findings
    );
    let conn = Connection::open(&out).unwrap();
    let eval_hits =
        count_where(&conn, tables::FINDINGS, cols::findings::RULE_ID, "python-eval-use");
    assert!(eval_hits >= 1, "python-eval-use hit at least once, got {eval_hits}");
    let exec_hits =
        count_where(&conn, tables::FINDINGS, cols::findings::RULE_ID, "python-exec-use");
    assert!(exec_hits >= 1, "python-exec-use hit at least once, got {exec_hits}");
    let print_hits =
        count_where(&conn, tables::FINDINGS, cols::findings::RULE_ID, "python-print-call");
    assert!(print_hits >= 1, "python-print-call hit at least once, got {print_hits}");
}

#[test]
fn python_index_records_files_and_dispatches_extractor() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("python.duckdb");
    let summary = build(python_request(out.clone())).unwrap();
    assert!(out.exists(), "python index file written");
    let conn = Connection::open(&out).unwrap();
    let py_files = count_where(&conn, tables::FILES, cols::files::LANGUAGE, "python");
    assert!(
        py_files >= 2,
        "expected at least 2 python files indexed, got {py_files}"
    );
    // Sanity: summary counters reflect at least the fixture's symbols.
    assert!(
        summary.counters.symbols >= 4,
        "expected ≥4 symbols (double, Counter, __init__, bump), got {}",
        summary.counters.symbols
    );
}

#[test]
fn python_index_produces_calls_and_imports() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("python-edges.duckdb");
    build(python_request(out.clone())).unwrap();
    let conn = Connection::open(&out).unwrap();

    let calls = count_where(&conn, tables::EDGES, cols::edges::KIND, "calls");
    assert!(
        calls >= 2,
        "expected ≥2 calls (Counter() + double(self.count) + double(c.count) + ping(); intra/cross-file), got {calls}"
    );

    let imports = count_where(&conn, tables::EDGES, cols::edges::KIND, "imports");
    assert!(
        imports >= 1,
        "expected ≥1 imports edge (app.py imports from lib), got {imports}"
    );
}

#[test]
fn python_extracts_functions_methods_and_classes() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("python-symbols.duckdb");
    build(python_request(out.clone())).unwrap();
    let conn = Connection::open(&out).unwrap();

    // double + main from lib.py + app.py are top-level functions.
    let functions = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "function");
    assert!(
        functions >= 2,
        "expected ≥2 top-level functions (double, main), got {functions}"
    );

    // __init__ + bump on Counter become Method (in_class).
    let methods = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "method");
    assert!(
        methods >= 2,
        "expected ≥2 methods (__init__, bump), got {methods}"
    );

    // `class Counter` maps to SymbolKind::Struct (per ADR-0012 mapping).
    let classes = count_where(&conn, tables::SYMBOLS, cols::symbols::KIND, "struct");
    assert!(
        classes >= 1,
        "expected ≥1 class symbol (Counter), got {classes}"
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
