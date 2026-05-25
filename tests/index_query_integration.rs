use std::path::PathBuf;

use duckdb::Connection;
use mallard::{
    BuildRequest, Direction, EdgeConfidence, EdgeKind, FindingFilter, IndexReader, MallardError,
    QueryRequest, QueryResult, SymbolId, build,
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

fn build_python_fixture(out: &PathBuf) {
    let req = BuildRequest {
        root: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("sample-python"),
        sha: "py-fixture".to_string(),
        rules_path: None,
        out_path: out.clone(),
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["python".to_string()],
        slowest_files_n: 10,
    };
    build(req).unwrap();
}

fn build_typescript_fixture(out: &PathBuf) {
    let req = BuildRequest {
        root: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("sample-typescript"),
        sha: "ts-fixture".to_string(),
        rules_path: None,
        out_path: out.clone(),
        max_file_bytes: 1024 * 1024,
        language_allow_list: vec!["typescript".to_string(), "tsx".to_string()],
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
    assert_eq!(meta.index_format_version, 2);
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
        .findings(&FindingFilter::default())
        .unwrap();
    assert!(!all.is_empty());

    let format_only = open_reader(&out)
        .findings(&FindingFilter {
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
        .findings(&FindingFilter {
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
        .findings(&FindingFilter {
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
fn cross_file_calls_resolve_after_build() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    // greet is defined in greet.rs and called from main.rs — exercises
    // the post-write resolver.
    let greet_id = find_symbol(&out, "greet.rs", "greet");
    let reader = open_reader(&out);
    let callers = reader
        .neighbors(&greet_id, &[EdgeKind::Calls], Direction::In)
        .unwrap();

    let cross_file_caller = callers.iter().find(|e| {
        e.src.path == "main.rs"
            && e.dst.as_ref().map(|d| d.qualified_name == "greet").unwrap_or(false)
    });
    assert!(
        cross_file_caller.is_some(),
        "expected resolved cross-file call from main.rs into greet.rs::greet, got {callers:?}"
    );
}

#[test]
fn edges_by_file_returns_bundle_per_symbol_with_edges() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let reader = open_reader(&out);
    let bundles = reader
        .edges_by_file("lib.rs", &[EdgeKind::Calls], Direction::Both)
        .unwrap();

    let bump = bundles
        .iter()
        .find(|b| b.symbol.qualified_name == "Counter::bump")
        .expect("Counter::bump bundle present");
    let calls_double = bump.outbound.iter().any(|e| {
        e.dst.as_ref().map(|d| d.qualified_name == "double").unwrap_or(false)
            || e.dst_unresolved.as_deref() == Some("double")
    });
    assert!(
        calls_double,
        "Counter::bump outbound should mention double, got {:?}",
        bump.outbound
    );
}

#[test]
fn edges_by_file_preserves_symbols_with_zero_edges() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let reader = open_reader(&out);
    let bundles = reader
        .edges_by_file("lib.rs", &[EdgeKind::Calls], Direction::Both)
        .unwrap();

    // The Counter struct itself is not callable; its bundle should still be
    // present with empty outbound/inbound on `calls`.
    let counter = bundles
        .iter()
        .find(|b| b.symbol.qualified_name == "Counter")
        .expect("Counter bundle present");
    assert!(counter.outbound.is_empty());
    assert!(counter.inbound.is_empty());
}

#[test]
fn neighbors_carry_edge_confidence() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    // bump → double is an intra-file resolution → extracted.
    let bump = find_symbol(&out, "lib.rs", "Counter::bump");
    let edges = open_reader(&out)
        .neighbors(&bump, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    let to_double = edges
        .iter()
        .find(|e| {
            e.dst.as_ref().map(|d| d.qualified_name == "double").unwrap_or(false)
        })
        .expect("bump → double edge present");
    assert_eq!(to_double.confidence, EdgeConfidence::Extracted);
}

#[test]
fn method_call_on_self_field_does_not_claim_extracted() {
    // Regression: `self.<field>.<method>()` previously resolved to a same-impl
    // method with the same short name and asserted confidence=Extracted (a
    // self-recursion claim). The fix demotes such calls so the post-build
    // resolver tiers them as Ambiguous or Inferred.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let outer_ping = find_symbol(&out, "wrapper.rs", "Outer::ping");
    let outbound = open_reader(&out)
        .neighbors(&outer_ping, &[EdgeKind::Calls], Direction::Out)
        .unwrap();

    // No outbound edge should be Extracted to Outer::ping itself.
    let bad = outbound.iter().find(|e| {
        e.confidence == EdgeConfidence::Extracted
            && e.dst.as_ref().map(|d| d.qualified_name == "Outer::ping").unwrap_or(false)
    });
    assert!(bad.is_none(), "self-recursion claim present: {bad:?}");

    // With two `ping` symbols in the file, the resolver marks this Ambiguous.
    let ping_edge = outbound
        .iter()
        .find(|e| {
            e.dst_unresolved.as_deref() == Some("ping")
                || e.dst.as_ref().map(|d| d.qualified_name == "Inner::ping").unwrap_or(false)
        })
        .expect("ping call edge present");
    assert!(
        matches!(
            ping_edge.confidence,
            EdgeConfidence::Ambiguous | EdgeConfidence::Inferred
        ),
        "expected Ambiguous|Inferred for self.<field>.<method>, got {:?}",
        ping_edge.confidence,
    );
}

#[test]
fn nested_macro_in_allowlisted_body_does_not_emit_phantom_call() {
    // Regression (C1): `format!(...)` nested inside `assert_eq!(...)` must
    // not produce a `Calls(format)` edge. The `!` between identifier and
    // token_tree marks it as a macro invocation, not a function call.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let caller = find_symbol(&out, "wrapper.rs", "tests::nested_macro_must_not_phantom_call");
    let outbound = open_reader(&out)
        .neighbors(&caller, &[EdgeKind::Calls], Direction::Out)
        .unwrap();

    let bad = outbound.iter().find(|e| {
        e.dst_unresolved.as_deref() == Some("format")
            || e.dst.as_ref().map(|d| d.qualified_name == "format").unwrap_or(false)
    });
    assert!(bad.is_none(), "nested macro emitted as phantom Calls edge: {bad:?}");
}

#[test]
fn macro_body_type_qualified_call_preserves_qualifier() {
    // Regression (C6): `Builder::make()` inside `assert!(...)` must emit
    // the qualified name `Builder::make`, not bare `make`. The walker
    // prepends the prior identifier when it crosses anonymous `::`. The
    // post-build resolver then matches against `by_qualified` and writes
    // a resolved Inferred edge to the real `Builder::make` symbol.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let caller = find_symbol(
        &out,
        "wrapper.rs",
        "tests::type_qualified_call_in_macro_keeps_qualifier",
    );
    let outbound = open_reader(&out)
        .neighbors(&caller, &[EdgeKind::Calls], Direction::Out)
        .unwrap();

    let resolved = outbound.iter().find(|e| {
        e.dst
            .as_ref()
            .map(|d| d.qualified_name == "Builder::make")
            .unwrap_or(false)
    });
    assert!(
        resolved.is_some(),
        "expected resolved Builder::make edge (via by_qualified); got {outbound:?}"
    );
}

#[test]
fn macro_body_method_position_call_is_ambiguous_not_inferred() {
    // Regression (C3): `o.ping()` inside an `assert!(...)` macro body has
    // no recoverable receiver type. The parser emits it as Ambiguous so
    // the resolver does NOT promote it to Inferred against globally-unique
    // unrelated symbols. UnresolvedCallerHit retains the Ambiguous tier.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let hits = open_reader(&out)
        .unresolved_callers(&["ping".to_string()], &[EdgeKind::Calls])
        .unwrap();
    let assert_hit = hits
        .iter()
        .find(|h| h.caller.qualified_name == "tests::ping_via_assert")
        .expect("ping call from assert! body present");
    assert_eq!(
        assert_hit.confidence,
        EdgeConfidence::Ambiguous,
        "method-position macro-body call must stay Ambiguous"
    );
}

#[test]
fn macro_body_method_call_is_visible_to_unresolved_callers() {
    // Regression: tree-sitter parses macro bodies as opaque `token_tree`
    // nodes. `assert!(o.ping() > 0)` previously hid the `ping` call site —
    // `unresolved-callers --name ping` returned zero hits. The macro-body
    // extractor (Gap 3) walks the token_tree and emits each `name(args)`
    // shape as an Unresolved-trust call reference, which the resolver tiers.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let hits = open_reader(&out)
        .unresolved_callers(&["ping".to_string()], &[EdgeKind::Calls])
        .unwrap();
    let from_assert = hits
        .iter()
        .find(|h| h.caller.qualified_name == "tests::ping_via_assert");
    assert!(
        from_assert.is_some(),
        "expected ping call from tests::ping_via_assert (inside assert! body); got {hits:?}"
    );
}

#[test]
fn method_call_on_bare_self_stays_extracted() {
    // Counter-test: bare `self.<method>()` is a same-impl-block call. The
    // intra-file map is the right resolution, confidence Extracted.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let echo = find_symbol(&out, "wrapper.rs", "Outer::echo");
    let outbound = open_reader(&out)
        .neighbors(&echo, &[EdgeKind::Calls], Direction::Out)
        .unwrap();

    let to_outer_ping = outbound
        .iter()
        .find(|e| e.dst.as_ref().map(|d| d.qualified_name == "Outer::ping").unwrap_or(false))
        .expect("echo → Outer::ping edge present");
    assert_eq!(to_outer_ping.confidence, EdgeConfidence::Extracted);
}

#[test]
fn bare_name_call_does_not_resolve_to_method() {
    // Regression (C2): a bare-name call cannot reach a `&self` method
    // without a receiver. `solo` exists only as `OnlyMethod::solo` (Method
    // kind) in wrapper.rs; the bare `solo()` callsite must not emit an
    // Extracted edge to that method.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let caller = find_symbol(&out, "wrapper.rs", "bare_solo_must_not_resolve_to_method");
    let outbound = open_reader(&out)
        .neighbors(&caller, &[EdgeKind::Calls], Direction::Out)
        .unwrap();

    let bad = outbound.iter().find(|e| {
        e.confidence == EdgeConfidence::Extracted
            && e.dst
                .as_ref()
                .map(|d| d.qualified_name == "OnlyMethod::solo")
                .unwrap_or(false)
    });
    assert!(bad.is_none(), "bare-name call falsely Extracted to method: {bad:?}");
}

#[test]
fn bare_call_to_const_fn_pointer_stays_extracted() {
    // Regression (C7): a `const HANDLER: fn() = ...` is callable via
    // `HANDLER()`. Previous candidate-kind filter (Function | Macro)
    // dropped Const, demoting the edge to Unresolved.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let caller = find_symbol(&out, "wrapper.rs", "bare_caller_of_const_callable");
    let outbound = open_reader(&out)
        .neighbors(&caller, &[EdgeKind::Calls], Direction::Out)
        .unwrap();

    let to_const = outbound
        .iter()
        .find(|e| {
            e.dst
                .as_ref()
                .map(|d| d.qualified_name == "CONST_CALLABLE")
                .unwrap_or(false)
        })
        .expect("bare CONST_CALLABLE() edge present");
    assert_eq!(to_const.confidence, EdgeConfidence::Extracted);
}

#[test]
fn inherent_plus_trait_impl_same_method_stays_extracted() {
    // Regression (C4): inherent and trait impls of the same `Foo::method`
    // both produce candidates with qualified_name `Foo::method`. Without
    // dedupe by qualified_name in pick_extracted_target, matching.len()==2
    // demotes the bare-self call to Unresolved.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let show_tag = find_symbol(&out, "wrapper.rs", "Outer::show_tag");
    let outbound = open_reader(&out)
        .neighbors(&show_tag, &[EdgeKind::Calls], Direction::Out)
        .unwrap();

    let to_tag = outbound
        .iter()
        .find(|e| e.dst.as_ref().map(|d| d.qualified_name == "Outer::tag").unwrap_or(false))
        .expect("show_tag → Outer::tag edge present");
    assert_eq!(
        to_tag.confidence,
        EdgeConfidence::Extracted,
        "bare self.tag() must collapse inherent+trait `Outer::tag` to one Extracted target"
    );
}

#[test]
fn typescript_method_call_on_this_field_does_not_claim_extracted() {
    // Gap 2 / C4 port: `this.inner.ping()` from Outer.ping must NOT
    // resolve to Outer.ping. Receiver is `this.inner` (Inner), not bare
    // `this`. Two `ping` methods → resolver tiers Ambiguous.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("ts-wrapper.duckdb");
    build_typescript_fixture(&out);
    let outer_ping = find_symbol(&out, "wrapper.ts", "Outer.ping");
    let outbound = open_reader(&out)
        .neighbors(&outer_ping, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    let bad = outbound.iter().find(|e| {
        e.confidence == EdgeConfidence::Extracted
            && e.dst.as_ref().map(|d| d.qualified_name == "Outer.ping").unwrap_or(false)
    });
    assert!(bad.is_none(), "this-recursion claim present: {bad:?}");
    let ping_edge = outbound
        .iter()
        .find(|e| {
            e.dst_unresolved.as_deref() == Some("ping")
                || e.dst.as_ref().map(|d| d.qualified_name == "Inner.ping").unwrap_or(false)
        })
        .expect("ping call edge present");
    assert!(matches!(
        ping_edge.confidence,
        EdgeConfidence::Ambiguous | EdgeConfidence::Inferred
    ));
}

#[test]
fn typescript_method_call_on_bare_this_stays_extracted() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("ts-wrapper-echo.duckdb");
    build_typescript_fixture(&out);
    let echo = find_symbol(&out, "wrapper.ts", "Outer.echo");
    let outbound = open_reader(&out)
        .neighbors(&echo, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    let to_outer_ping = outbound
        .iter()
        .find(|e| e.dst.as_ref().map(|d| d.qualified_name == "Outer.ping").unwrap_or(false))
        .expect("echo → Outer.ping edge present");
    assert_eq!(to_outer_ping.confidence, EdgeConfidence::Extracted);
}

#[test]
fn typescript_bare_name_call_does_not_resolve_to_method() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("ts-wrapper-solo.duckdb");
    build_typescript_fixture(&out);
    let caller = find_symbol(&out, "wrapper.ts", "bareSoloMustNotResolveToMethod");
    let outbound = open_reader(&out)
        .neighbors(&caller, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    let bad = outbound.iter().find(|e| {
        e.confidence == EdgeConfidence::Extracted
            && e.dst.as_ref().map(|d| d.qualified_name == "OnlyMethod.solo").unwrap_or(false)
    });
    assert!(bad.is_none(), "bare-name call falsely Extracted to method: {bad:?}");
}

#[test]
fn python_method_call_on_self_field_does_not_claim_extracted() {
    // Gap 2 port: `self.inner.ping()` from Outer.ping must NOT resolve
    // to Outer.ping (self-recursion). The resolver tiers as Ambiguous
    // because two `ping` methods exist (Inner.ping + Outer.ping).
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("py-wrapper.duckdb");
    build_python_fixture(&out);
    let outer_ping = find_symbol(&out, "wrapper.py", "Outer.ping");
    let outbound = open_reader(&out)
        .neighbors(&outer_ping, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    let bad = outbound.iter().find(|e| {
        e.confidence == EdgeConfidence::Extracted
            && e.dst.as_ref().map(|d| d.qualified_name == "Outer.ping").unwrap_or(false)
    });
    assert!(bad.is_none(), "self-recursion claim present: {bad:?}");
    let ping_edge = outbound
        .iter()
        .find(|e| {
            e.dst_unresolved.as_deref() == Some("ping")
                || e.dst.as_ref().map(|d| d.qualified_name == "Inner.ping").unwrap_or(false)
        })
        .expect("ping call edge present");
    assert!(matches!(
        ping_edge.confidence,
        EdgeConfidence::Ambiguous | EdgeConfidence::Inferred
    ));
}

#[test]
fn python_method_call_on_bare_self_stays_extracted() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("py-wrapper-echo.duckdb");
    build_python_fixture(&out);
    let echo = find_symbol(&out, "wrapper.py", "Outer.echo");
    let outbound = open_reader(&out)
        .neighbors(&echo, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    let to_outer_ping = outbound
        .iter()
        .find(|e| e.dst.as_ref().map(|d| d.qualified_name == "Outer.ping").unwrap_or(false))
        .expect("echo → Outer.ping edge present");
    assert_eq!(to_outer_ping.confidence, EdgeConfidence::Extracted);
}

#[test]
fn python_bare_name_call_does_not_resolve_to_method() {
    // C2 port: `solo` exists only as `OnlyMethod.solo` (Method). A bare
    // `solo()` callsite must not claim Extracted on that method.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("py-wrapper-solo.duckdb");
    build_python_fixture(&out);
    let caller = find_symbol(&out, "wrapper.py", "bare_solo_must_not_resolve_to_method");
    let outbound = open_reader(&out)
        .neighbors(&caller, &[EdgeKind::Calls], Direction::Out)
        .unwrap();
    let bad = outbound.iter().find(|e| {
        e.confidence == EdgeConfidence::Extracted
            && e.dst
                .as_ref()
                .map(|d| d.qualified_name == "OnlyMethod.solo")
                .unwrap_or(false)
    });
    assert!(bad.is_none(), "bare-name call falsely Extracted to method: {bad:?}");
}

#[test]
fn cross_file_resolved_edges_are_inferred() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    // main → greet is a cross-file call resolved by the post-build pass.
    let greet = find_symbol(&out, "greet.rs", "greet");
    let callers = open_reader(&out)
        .neighbors(&greet, &[EdgeKind::Calls], Direction::In)
        .unwrap();
    let cross_file = callers
        .iter()
        .find(|e| e.src.path == "main.rs")
        .expect("main.rs caller present");
    assert_eq!(cross_file.confidence, EdgeConfidence::Inferred);
}

#[test]
fn unresolved_callers_unknown_name_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    // Fixture is small and self-contained; the post-build resolver picks
    // up every cross-file call. A name that nothing references must
    // return no hits.
    let hits = open_reader(&out)
        .unresolved_callers(
            &["definitely_not_a_real_symbol_xyz".to_string()],
            &[EdgeKind::Calls],
        )
        .unwrap();
    assert!(hits.is_empty(), "got {hits:?}");
}

#[test]
fn unresolved_callers_empty_names_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let hits = open_reader(&out)
        .unresolved_callers(&[], &[])
        .unwrap();
    assert!(hits.is_empty());
}

#[test]
fn edges_by_file_empty_file_path_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("index.duckdb");
    build_fixture(&out, false);

    let bundles = open_reader(&out)
        .edges_by_file("no/such/file.rs", &[], Direction::Both)
        .unwrap();
    assert!(bundles.is_empty());
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
            assert_eq!(expected, 2);
        }
        other => panic!("expected VersionMismatch, got {other:?}"),
    }
}
