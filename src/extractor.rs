use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use crate::core::{
    Anchor, Edge, EdgeConfidence, EdgeKind, FileId, ParseError, ParsedFile, Result, Symbol,
    SymbolId, SymbolKind,
};
use crate::parsed_source::ParsedSource;

/// Per-language adapter that turns a `ParsedSource` into a `ParsedFile`
/// (symbols + edges + parse errors). The seam where second-language support
/// will slot in — see CONTEXT.md.
pub trait SymbolExtractor: Send {
    fn extract(&mut self, parsed: &ParsedSource) -> ParsedFile;
}

const RUST_QUERY: &str = r#"
(function_item name: (identifier) @def.function.name) @def.function

(struct_item name: (type_identifier) @def.struct.name) @def.struct

(enum_item name: (type_identifier) @def.enum.name) @def.enum

(trait_item name: (type_identifier) @def.trait.name) @def.trait

(mod_item name: (identifier) @def.module.name) @def.module

(const_item name: (identifier) @def.const.name) @def.const

(static_item name: (identifier) @def.static.name) @def.static

(type_item name: (type_identifier) @def.type_alias.name) @def.type_alias

(macro_definition name: (identifier) @def.macro.name) @def.macro

(call_expression function: (identifier) @ref.call.simple) @ref.call

(call_expression function: (field_expression field: (field_identifier) @ref.call.method)) @ref.call

(call_expression function: (scoped_identifier name: (identifier) @ref.call.scoped)) @ref.call

(macro_invocation macro: (identifier) @ref.macro.name) @ref.macro

(macro_invocation macro: (scoped_identifier name: (identifier) @ref.macro.name)) @ref.macro

(use_declaration) @import.decl
"#;

pub struct RustExtractor {
    query: Query,
    cursor: QueryCursor,
}

impl RustExtractor {
    pub fn new() -> Result<Self> {
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let query = Query::new(&language, RUST_QUERY)?;
        Ok(RustExtractor {
            query,
            cursor: QueryCursor::new(),
        })
    }

    fn run_query(
        &mut self,
        root: Node<'_>,
        source: &[u8],
        file_id: FileId,
        relative_path: &str,
    ) -> (Vec<Symbol>, Vec<Edge>) {
        let mut symbols: Vec<Symbol> = Vec::new();
        let mut references: Vec<CallRef> = Vec::new();
        let mut imports: Vec<(Node, String)> = Vec::new();

        let mut matches = self.cursor.matches(&self.query, root, source);
        while let Some(m) = matches.next() {
            let pattern = m.pattern_index;
            match pattern {
                0..=8 => {
                    if let Some(sym) = build_symbol_match(m, pattern, source, file_id, relative_path) {
                        symbols.push(sym);
                    }
                }
                9..=11 => {
                    if let Some((node, name, trust_intra_file)) = ref_call_match(m, source) {
                        references.push(CallRef {
                            node,
                            name,
                            kind: EdgeKind::Calls,
                            trust_intra_file,
                            inhibit_resolver: false,
                        });
                    }
                }
                12 | 13 => {
                    if let Some((macro_name, macro_invocation)) = macro_invocation_match(m, source)
                        && MACRO_BODY_EXPRESSION_ALLOWLIST.contains(&macro_name.as_str())
                    {
                        collect_macro_body_calls(macro_invocation, source, &mut references);
                    }
                }
                14 => {
                    if let Some(node) = m.captures.first().map(|c| c.node) {
                        let text = node_text(node, source);
                        imports.push((node, text));
                    }
                }
                _ => {}
            }
        }

        let mut edges: Vec<Edge> = Vec::new();

        let mut symbols_by_short: std::collections::HashMap<&str, Vec<&Symbol>> =
            std::collections::HashMap::with_capacity(symbols.len());
        for s in &symbols {
            let key = short_name(&s.qualified_name).unwrap_or(s.qualified_name.as_str());
            symbols_by_short.entry(key).or_default().push(s);
        }

        let file_pseudo_src = SymbolId(format!("file:{}", relative_path));
        for sym in &symbols {
            edges.push(Edge {
                src: file_pseudo_src.clone(),
                dst: Some(sym.id.clone()),
                dst_unresolved: None,
                kind: EdgeKind::Contains,
                confidence: EdgeConfidence::Extracted,
                file_id,
                order_key: sym.anchor.start_byte,
            });
        }

        // Filter out tuple-struct / enum-variant constructor calls. Rust's
        // call_expression grammar can't distinguish `Ok(x)` (variant) from
        // `f(x)` (function) without name resolution — heuristic: drop if the
        // call name is a stdlib variant constructor, matches a same-file
        // type definition, or is PascalCase with no same-file function /
        // method / macro of the same name.
        for r in references {
            if r.kind == EdgeKind::Calls && is_constructor_call(&r.name, &symbols_by_short) {
                continue;
            }
            let enclosing_sym = find_enclosing_definition(r.node, &symbols);
            let enclosing = enclosing_sym
                .map(|s| s.id.clone())
                .unwrap_or_else(|| file_pseudo_src.clone());
            // Only claim Extracted when the call's receiver semantics let us
            // trust the intra-file short-name map; otherwise emit Unresolved
            // and let the post-build resolver tier (Inferred / Ambiguous /
            // Unresolved). See ADR-0010 and `ref_call_match`.
            let dst = if r.trust_intra_file {
                let candidates = symbols_by_short
                    .get(r.name.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                pick_extracted_target(candidates, r.node, enclosing_sym).map(|s| s.id.clone())
            } else {
                None
            };
            // `inhibit_resolver=true` (macro-body method-position calls whose
            // receiver type can't be recovered from the token_tree) emit at
            // confidence Ambiguous so the resolver leaves them alone — it
            // promotes Unresolved only. Resolver skip in `store.rs::resolve_edges`.
            let (dst_unresolved, confidence) = if dst.is_some() {
                (None, EdgeConfidence::Extracted)
            } else if r.inhibit_resolver {
                (Some(r.name.clone()), EdgeConfidence::Ambiguous)
            } else {
                (Some(r.name.clone()), EdgeConfidence::Unresolved)
            };
            edges.push(Edge {
                src: enclosing,
                dst,
                dst_unresolved,
                kind: r.kind,
                confidence,
                file_id,
                order_key: r.node.start_byte() as u64,
            });
        }

        for (node, import_text) in imports {
            edges.push(Edge {
                src: file_pseudo_src.clone(),
                dst: None,
                dst_unresolved: Some(import_text),
                kind: EdgeKind::Imports,
                confidence: EdgeConfidence::Unresolved,
                file_id,
                order_key: node.start_byte() as u64,
            });
        }

        symbols.sort_by_key(|s| s.anchor.start_byte);
        edges.sort_by_key(|e| (e.kind.as_str(), e.order_key));

        (symbols, edges)
    }
}

impl SymbolExtractor for RustExtractor {
    fn extract(&mut self, parsed: &ParsedSource) -> ParsedFile {
        let source = parsed.source().as_bytes();
        let root = parsed.ts_root();
        let file_id = parsed.file_id();
        let relative_path = parsed.relative_path();

        let mut parse_errors: Vec<ParseError> = Vec::new();
        if root.has_error() {
            collect_errors(root, source, file_id, &mut parse_errors);
        }

        let t_query = std::time::Instant::now();
        let (symbols, edges) = self.run_query(root, source, file_id, relative_path);
        let query_ms = t_query.elapsed().as_millis() as u64;

        ParsedFile {
            file_id,
            symbols,
            edges,
            parse_errors,
            parse_ms: parsed.parse_ms,
            query_ms,
        }
    }
}

fn build_symbol_match<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    pattern: usize,
    source: &[u8],
    file_id: FileId,
    relative_path: &str,
) -> Option<Symbol> {
    let kind = match pattern {
        0 => SymbolKind::Function,
        1 => SymbolKind::Struct,
        2 => SymbolKind::Enum,
        3 => SymbolKind::Trait,
        4 => SymbolKind::Module,
        5 => SymbolKind::Const,
        6 => SymbolKind::Static,
        7 => SymbolKind::TypeAlias,
        8 => SymbolKind::Macro,
        _ => return None,
    };
    if m.captures.len() < 2 {
        return None;
    }
    let (name_node, def_node) = pick_name_and_def(m)?;
    if matches!(kind, SymbolKind::TypeAlias) && is_method(def_node) {
        return None;
    }
    let name = node_text(name_node, source);
    let qualified_name = compute_qualified_name(def_node, &name, kind, source);
    let signature = compute_signature(def_node, source, kind);
    let anchor = node_anchor(def_node);
    let id = SymbolId::compute(relative_path, &qualified_name, kind, &signature);

    let final_kind = if matches!(kind, SymbolKind::Function) && is_method(def_node) {
        SymbolKind::Method
    } else {
        kind
    };

    Some(Symbol {
        id,
        file_id,
        qualified_name,
        kind: final_kind,
        signature,
        anchor,
    })
}

fn pick_name_and_def<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
) -> Option<(Node<'tree>, Node<'tree>)> {
    let mut smallest: Option<Node> = None;
    let mut largest: Option<Node> = None;
    for c in m.captures {
        let n = c.node;
        let span = n.end_byte().saturating_sub(n.start_byte());
        match smallest {
            None => smallest = Some(n),
            Some(s) if span < s.end_byte().saturating_sub(s.start_byte()) => smallest = Some(n),
            _ => {}
        }
        match largest {
            None => largest = Some(n),
            Some(l) if span > l.end_byte().saturating_sub(l.start_byte()) => largest = Some(n),
            _ => {}
        }
    }
    Some((smallest?, largest?))
}

/// Extract `(macro_name, macro_invocation_node)` from a pattern-12/13 match.
/// The macro name is the text of the `@ref.macro.name` identifier capture;
/// the macro_invocation node is the largest `@ref.macro` capture.
fn macro_invocation_match<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    source: &[u8],
) -> Option<(String, Node<'tree>)> {
    let name_capture = m
        .captures
        .iter()
        .find(|c| c.node.kind() == "identifier")?;
    let invocation = m
        .captures
        .iter()
        .map(|c| c.node)
        .find(|n| n.kind() == "macro_invocation")?;
    Some((node_text(name_capture.node, source), invocation))
}

/// References emitted to the references vec carry per-edge hints that drive
/// downstream emission: whether the parser is allowed to trust the per-file
/// short-name map for Extracted promotion, and whether the post-build
/// resolver should leave the edge alone (i.e. the parser already knows the
/// edge is ambiguous because receiver context was lost — see C3).
struct CallRef<'tree> {
    node: Node<'tree>,
    name: String,
    kind: EdgeKind,
    trust_intra_file: bool,
    inhibit_resolver: bool,
}

/// Cap recursion depth in `walk_token_tree_for_calls` so pathological macro
/// inputs (nested parens crafted by adversarial source) can't overflow the
/// thread stack. Real Rust hits depths in the low double digits; 64 is
/// comfortable headroom without being a usability foot-gun.
const MACRO_BODY_MAX_DEPTH: u32 = 64;

/// Walk a macro_invocation's `token_tree` and emit a call reference for each
/// `name(args)` shape we can recognise inside the unparsed body. Receiver
/// chains aren't tracked, so emitted refs always set `trust_intra_file =
/// false`. The walker classifies each call site:
///
/// - identifier preceded by anonymous `.` → method position; we can't know
///   the receiver type, so emit with `inhibit_resolver = true` (the parser
///   tier marks Ambiguous; the resolver doesn't touch it).
/// - identifier preceded by anonymous `::` and a prior identifier → emit
///   the qualified name `Type::method`, letting the resolver use its
///   `by_qualified` index.
/// - identifier followed by anonymous `!` → nested macro invocation inside
///   the outer macro body; skip (tree-sitter doesn't re-parse the inner
///   macro, but the surrounding `identifier`-then-`token_tree` shape would
///   otherwise be misclassified as a fn call).
/// - everything else → free-function shape, emit Unresolved.
fn collect_macro_body_calls<'tree>(
    macro_invocation: Node<'tree>,
    source: &[u8],
    out: &mut Vec<CallRef<'tree>>,
) {
    let Some(token_tree) = macro_invocation
        .named_children(&mut macro_invocation.walk())
        .find(|c| c.kind() == "token_tree")
    else {
        return;
    };
    walk_token_tree_for_calls(token_tree, source, out, 0);
}

fn walk_token_tree_for_calls<'tree>(
    token_tree: Node<'tree>,
    source: &[u8],
    out: &mut Vec<CallRef<'tree>>,
    depth: u32,
) {
    if depth >= MACRO_BODY_MAX_DEPTH {
        return;
    }
    // Single forward pass: remember the most recent `identifier` child and
    // emit a call ref when the next named sibling is a `token_tree` (the
    // `name(args)` shape). Recurse into nested token_trees.
    let mut cursor = token_tree.walk();
    let mut pending_ident: Option<Node<'tree>> = None;
    for child in token_tree.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => pending_ident = Some(child),
            "token_tree" => {
                if let Some(ident) = pending_ident.take() {
                    emit_macro_body_call(ident, source, out);
                }
                walk_token_tree_for_calls(child, source, out, depth + 1);
            }
            _ => pending_ident = None,
        }
    }
}

/// Classify an `identifier`-then-`token_tree` pair inside a macro body and
/// push an appropriate `CallRef`. Anonymous siblings of the identifier
/// (`!`, `.`, `::`) disambiguate the call site shape — see the doc on
/// `collect_macro_body_calls`.
fn emit_macro_body_call<'tree>(
    ident: Node<'tree>,
    source: &[u8],
    out: &mut Vec<CallRef<'tree>>,
) {
    // Nested macro invocation: identifier immediately followed (including
    // anonymous siblings) by `!`, then `token_tree`. Skip — it's not a fn
    // call, and the inner macro body isn't parsed.
    if ident
        .next_sibling()
        .is_some_and(|n| n.kind() == "!")
    {
        return;
    }

    let prev = ident.prev_sibling();

    // `Type::method(...)` shape — emit qualified name. Walk back across the
    // anonymous `::` to the prior identifier (which is the Type segment).
    if let Some(p) = prev
        && p.kind() == "::"
        && let Some(type_ident) = p.prev_sibling()
        && type_ident.kind() == "identifier"
    {
        let type_name = node_text(type_ident, source);
        let method_name = node_text(ident, source);
        out.push(CallRef {
            node: ident,
            name: format!("{type_name}::{method_name}"),
            kind: EdgeKind::Calls,
            trust_intra_file: false,
            inhibit_resolver: false,
        });
        return;
    }

    // `receiver.method(...)` shape — method position. Receiver type isn't
    // tracked, so a globally-unique short name match would mis-Infer. Emit
    // with `inhibit_resolver = true` so the parser tier marks Ambiguous and
    // the post-build resolver leaves it alone.
    let method_position = prev.is_some_and(|p| p.kind() == ".");

    out.push(CallRef {
        node: ident,
        name: node_text(ident, source),
        kind: EdgeKind::Calls,
        trust_intra_file: false,
        inhibit_resolver: method_position,
    });
}

fn ref_call_match<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    source: &[u8],
) -> Option<(Node<'tree>, String, bool)> {
    let name_capture = m.captures.iter().rev().find(|c| {
        let kind = c.node.kind();
        kind == "identifier" || kind == "field_identifier"
    })?;
    let node = name_capture.node;
    let name = node_text(node, source);
    // Trust the per-file short-name map only when the receiver type matches
    // the enclosing impl block. For method calls (`field_identifier`), that's
    // true iff the receiver is bare `self` — `self.foo()` resolves within the
    // same impl, `self.field.foo()` does not. Bare-name (`identifier`) and
    // scoped (`Type::name`) calls have no receiver, so the intra-file map is
    // the right place to look.
    let trust_intra_file = if node.kind() == "field_identifier" {
        method_call_receiver_is_bare_self(node)
    } else {
        true
    };
    Some((node, name, trust_intra_file))
}

/// True when a `field_identifier` capture corresponds to a method call whose
/// receiver is bare `self`. Walks up to the enclosing `field_expression` and
/// inspects its `value` field.
fn method_call_receiver_is_bare_self(field_identifier: Node) -> bool {
    field_identifier
        .parent()
        .filter(|p| p.kind() == "field_expression")
        .and_then(|p| p.child_by_field_name("value"))
        .is_some_and(|recv| recv.kind() == "self")
}

fn compute_qualified_name(def_node: Node, name: &str, kind: SymbolKind, source: &[u8]) -> String {
    if matches!(kind, SymbolKind::Function) {
        if let Some(impl_type) = enclosing_impl_type(def_node, source) {
            return format!("{impl_type}::{name}");
        }
    }
    let modules = enclosing_module_path(def_node, source);
    if modules.is_empty() {
        name.to_string()
    } else {
        format!("{}::{name}", modules.join("::"))
    }
}

fn enclosing_module_path(def_node: Node, source: &[u8]) -> Vec<String> {
    let mut path: Vec<String> = Vec::new();
    let mut cur = def_node.parent();
    while let Some(p) = cur {
        if p.kind() == "mod_item" {
            if let Some(name_node) = p.child_by_field_name("name") {
                path.push(node_text(name_node, source));
            }
        }
        cur = p.parent();
    }
    path.reverse();
    path
}

fn compute_signature(def_node: Node, source: &[u8], kind: SymbolKind) -> String {
    if !matches!(kind, SymbolKind::Function) {
        return String::new();
    }
    let mut cursor = def_node.walk();
    for child in def_node.children(&mut cursor) {
        if child.kind() == "parameters" {
            return canonical_params(node_text(child, source));
        }
    }
    String::new()
}

// Rust stdlib variant constructors callable with bare-identifier syntax.
const STDLIB_VARIANT_CONSTRUCTORS: &[&str] = &["Ok", "Err", "Some", "None"];

// Macros whose token_tree body is expression syntax — i.e. calls inside them
// are real call sites. tree-sitter-rust does not parse macro bodies (they
// stay as flat `token_tree` nodes), so without an explicit descent these
// calls are invisible. Restricting to a known allowlist avoids descending
// into DSL macros whose tokens are not Rust expressions.
const MACRO_BODY_EXPRESSION_ALLOWLIST: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "dbg",
    "eprint",
    "eprintln",
    "format",
    "matches",
    "panic",
    "print",
    "println",
    "todo",
    "unimplemented",
    "unreachable",
    "write",
    "writeln",
];

fn is_constructor_call(
    name: &str,
    symbols_by_short: &std::collections::HashMap<&str, Vec<&Symbol>>,
) -> bool {
    if STDLIB_VARIANT_CONSTRUCTORS.contains(&name) {
        return true;
    }
    let candidates = symbols_by_short.get(name).map(Vec::as_slice).unwrap_or(&[]);
    let any_type_def = candidates.iter().any(|s| {
        matches!(
            s.kind,
            SymbolKind::Struct | SymbolKind::Enum | SymbolKind::Trait | SymbolKind::TypeAlias
        )
    });
    if any_type_def {
        return true;
    }
    // PascalCase identifiers that aren't a known callable in this file are
    // very likely scoped variant constructors (e.g. `QueryRequest::LookupSymbol(x)`
    // captures `LookupSymbol`). Rust functions and methods are snake_case;
    // SCREAMING_SNAKE_CASE consts are uppercase-leading but contain `_`, so
    // exclude them from the heuristic to keep `CONST_HANDLER()`-style calls.
    // Check the rightmost segment so qualified names from macro-body
    // extraction (`Builder::make`) aren't misclassified by their type prefix.
    let last_segment = name.rsplit("::").next().unwrap_or(name);
    let pascal_case = last_segment
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        && !last_segment.contains('_');
    if pascal_case {
        let any_callable = candidates.iter().any(|s| {
            matches!(
                s.kind,
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Macro
                    | SymbolKind::Const
                    | SymbolKind::Static
            )
        });
        if !any_callable {
            return true;
        }
    }
    false
}

/// Disambiguate intra-file short-name candidates for an Extracted match.
///
/// For a method call (`field_identifier` capture) we require the candidate to
/// share the caller's impl-type prefix — i.e. live in the same `impl` block.
/// For a bare-name function call (`identifier`) we accept only a unique
/// callable candidate; multiple shorts are ambiguous → let the resolver tier.
fn pick_extracted_target<'a>(
    candidates: &[&'a Symbol],
    call_node: Node,
    caller: Option<&Symbol>,
) -> Option<&'a Symbol> {
    if call_node.kind() == "field_identifier" {
        // Bare-self method call (ref_call_match enforces this). Accept a
        // candidate only if it lives in the caller's impl. Two impls of the
        // same `Foo::name` (inherent + trait) collide under the impl-type
        // prefix; dedupe by qualified_name so inherent/trait pairs collapse
        // to a single target rather than regressing to Unresolved.
        let caller_prefix = caller.and_then(|s| impl_type_prefix(&s.qualified_name))?;
        let matching: Vec<&Symbol> = candidates
            .iter()
            .copied()
            .filter(|s| impl_type_prefix(&s.qualified_name) == Some(caller_prefix))
            .collect();
        let distinct_qnames = matching
            .iter()
            .map(|s| s.qualified_name.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len();
        if distinct_qnames == 1 {
            matching.first().copied()
        } else {
            None
        }
    } else {
        // Bare-name (`identifier`) vs scoped (`Type::name`) — both leaf
        // captures are `identifier`-kind; parent distinguishes. A bare-name
        // call has no receiver and cannot reach a `&self` method; a scoped
        // call (UFCS / associated function) can. Restrict callable kinds
        // accordingly to avoid false-Extracted to methods from bare names.
        let is_scoped = call_node
            .parent()
            .is_some_and(|p| p.kind() == "scoped_identifier");
        // Const / Static can hold a fn-pointer value and are callable via
        // bare or scoped paths — `const HANDLER: fn() = my_handler;
        // HANDLER()`. Include them so the previously-Extracted edge to
        // such items survives the kind-filter introduced for C2.
        unique(candidates.iter().copied().filter(|s| {
            if is_scoped {
                matches!(
                    s.kind,
                    SymbolKind::Function
                        | SymbolKind::Method
                        | SymbolKind::Macro
                        | SymbolKind::Const
                        | SymbolKind::Static
                )
            } else {
                matches!(
                    s.kind,
                    SymbolKind::Function
                        | SymbolKind::Macro
                        | SymbolKind::Const
                        | SymbolKind::Static
                )
            }
        }))
    }
}

/// Return the sole item from `it` if it yields exactly one, else None.
fn unique<T, I: Iterator<Item = T>>(mut it: I) -> Option<T> {
    let first = it.next()?;
    if it.next().is_some() { None } else { Some(first) }
}

/// The `Type` portion of a `Type::method` qualified name. None for bare
/// (module-level) symbols.
fn impl_type_prefix(qualified: &str) -> Option<&str> {
    qualified.rsplit_once("::").map(|(prefix, _)| prefix)
}

fn canonical_params(text: String) -> String {
    let trimmed = text.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed);
    let normalized = inner.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = normalized.trim_end_matches(',').trim();
    format!("({normalized})")
}

fn is_method(def_node: Node) -> bool {
    let mut cur = def_node.parent();
    while let Some(p) = cur {
        if p.kind() == "impl_item" || p.kind() == "trait_item" {
            return true;
        }
        cur = p.parent();
    }
    false
}

fn enclosing_impl_type(def_node: Node, source: &[u8]) -> Option<String> {
    let mut cur = def_node.parent();
    while let Some(p) = cur {
        if p.kind() == "impl_item" {
            let type_node = p
                .child_by_field_name("type")
                .or_else(|| p.child_by_field_name("trait"))?;
            return Some(node_text(type_node, source));
        }
        if p.kind() == "trait_item" {
            return p
                .child_by_field_name("name")
                .map(|n| node_text(n, source));
        }
        cur = p.parent();
    }
    None
}

fn short_name(qualified: &str) -> Option<&str> {
    qualified.rsplit_once("::").map(|(_, last)| last)
}

fn find_enclosing_definition<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let start = node.start_byte() as u64;
    symbols
        .iter()
        .filter(|s| s.anchor.start_byte <= start && start < s.anchor.end_byte)
        .min_by_key(|s| s.anchor.end_byte - s.anchor.start_byte)
}

fn node_text(node: Node, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned()
}

fn node_anchor(node: Node) -> Anchor {
    Anchor {
        start_byte: node.start_byte() as u64,
        end_byte: node.end_byte() as u64,
        start_line: node.start_position().row as u32,
        end_line: node.end_position().row as u32,
    }
}

fn collect_errors(node: Node, source: &[u8], file_id: FileId, out: &mut Vec<ParseError>) {
    if node.is_error() || node.is_missing() {
        out.push(ParseError {
            file_id,
            message: if node.is_missing() {
                format!("missing node: {}", node.kind())
            } else {
                let snippet = node_text(node, source);
                let short = snippet.chars().take(64).collect::<String>();
                format!("syntax error near: {short}")
            },
            line: node.start_position().row as u32,
            col: node.start_position().column as u32,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.has_error() || child.is_error() || child.is_missing() {
            collect_errors(child, source, file_id, out);
        }
    }
}

