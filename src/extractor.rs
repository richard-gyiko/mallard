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
        let mut references: Vec<(Node, String, EdgeKind, bool)> = Vec::new();
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
                        references.push((node, name, EdgeKind::Calls, trust_intra_file));
                    }
                }
                12 => {
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
        for (node, name, kind, trust_intra_file) in references {
            if kind == EdgeKind::Calls && is_constructor_call(&name, &symbols_by_short) {
                continue;
            }
            let enclosing_sym = find_enclosing_definition(node, &symbols);
            let enclosing = enclosing_sym
                .map(|s| s.id.clone())
                .unwrap_or_else(|| file_pseudo_src.clone());
            // Only claim Extracted when the call's receiver semantics let us
            // trust the intra-file short-name map; otherwise emit Unresolved
            // and let the post-build resolver tier (Inferred / Ambiguous /
            // Unresolved). See ADR-0010 and `ref_call_match`.
            let dst = if trust_intra_file {
                let candidates = symbols_by_short
                    .get(name.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                pick_extracted_target(candidates, node, enclosing_sym).map(|s| s.id.clone())
            } else {
                None
            };
            let (dst_unresolved, confidence) = if dst.is_some() {
                (None, EdgeConfidence::Extracted)
            } else {
                (Some(name.clone()), EdgeConfidence::Unresolved)
            };
            edges.push(Edge {
                src: enclosing,
                dst,
                dst_unresolved,
                kind,
                confidence,
                file_id,
                order_key: node.start_byte() as u64,
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
    // PascalCase identifiers that aren't a known callable in this file are very
    // likely scoped variant constructors (e.g. `QueryRequest::LookupSymbol(x)`
    // captures `LookupSymbol`). Rust functions and methods are snake_case by
    // convention.
    let pascal_case = name
        .chars()
        .next()
        .map(|c| c.is_ascii_uppercase())
        .unwrap_or(false);
    if pascal_case {
        let any_callable = candidates.iter().any(|s| {
            matches!(
                s.kind,
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Macro
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
        unique(candidates.iter().copied().filter(|s| {
            if is_scoped {
                matches!(
                    s.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Macro
                )
            } else {
                matches!(s.kind, SymbolKind::Function | SymbolKind::Macro)
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

