use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use crate::core::{
    Edge, EdgeConfidence, EdgeKind, FileId, ParseError, ParsedFile, Result, Symbol, SymbolId,
    SymbolKind,
};
use crate::extractor::SymbolExtractor;
use crate::extractor_common::{
    DOT_SYNTAX, canonical_params, find_enclosing_definition, is_constructor_call, node_anchor,
    node_text, pick_method_target, pick_name_and_def, symbols_by_short, unique,
};
use crate::parse_errors;
use crate::parsed_source::ParsedSource;

const PYTHON_QUERY: &str = r#"
(function_definition name: (identifier) @def.function.name) @def.function

(class_definition name: (identifier) @def.class.name) @def.class

(call function: (identifier) @ref.call.simple) @ref.call

(call function: (attribute attribute: (identifier) @ref.call.method)) @ref.call

(import_statement) @import.decl

(import_from_statement) @import.decl
"#;

/// Python identifiers commonly used as the implicit receiver for instance
/// (`self`) or class (`cls`) methods. Bare-receiver method calls trust the
/// intra-file map; anything else (`self.<field>.method()`, `obj.method()`)
/// emits Unresolved so the post-build resolver can tier.
const PYTHON_BARE_RECEIVERS: &[&str] = &["self", "cls"];

/// Python `SymbolExtractor`. Mirrors the Rust extractor's shape — see
/// [`crate::extractor::RustExtractor`].
pub struct PythonExtractor {
    query: Query,
    cursor: QueryCursor,
}

impl PythonExtractor {
    pub fn new() -> Result<Self> {
        let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
        let query = Query::new(&language, PYTHON_QUERY)?;
        Ok(PythonExtractor {
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
            match m.pattern_index {
                0 | 1 => {
                    if let Some(sym) =
                        build_symbol_match(m, m.pattern_index, source, file_id, relative_path)
                    {
                        symbols.push(sym);
                    }
                }
                2 | 3 => {
                    if let Some(r) = ref_call_match(m, source) {
                        references.push(r);
                    }
                }
                4 | 5 => {
                    if let Some(node) = m.captures.first().map(|c| c.node) {
                        imports.push((node, node_text(node, source)));
                    }
                }
                _ => {}
            }
        }

        let file_pseudo_src = SymbolId(format!("file:{}", relative_path));
        let mut edges: Vec<Edge> = Vec::with_capacity(symbols.len() + references.len());
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

        let by_short = symbols_by_short(&symbols, DOT_SYNTAX.qname_sep);

        // Mirrors `RustExtractor` post wedge-dogfood-1 + C2/C4/C7 fixes:
        // class-constructor filter, per-call confidence tiering, and
        // impl-prefix scoping for bare-self method calls.
        for r in references {
            if is_constructor_call(
                &r.name,
                &by_short,
                DOT_SYNTAX.qname_sep,
                &[],
                is_py_type_kind,
            ) {
                continue;
            }
            let enclosing_sym = find_enclosing_definition(r.node, &symbols);
            let enclosing = enclosing_sym
                .map(|s| s.id.clone())
                .unwrap_or_else(|| file_pseudo_src.clone());
            let dst = if r.trust_intra_file {
                let candidates = by_short
                    .get(r.name.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                pick_extracted_target(candidates, r.node, enclosing_sym).map(|s| s.id.clone())
            } else {
                None
            };
            let (dst_unresolved, confidence) = if dst.is_some() {
                (None, EdgeConfidence::Extracted)
            } else {
                (Some(r.name.clone()), EdgeConfidence::Unresolved)
            };
            edges.push(Edge {
                src: enclosing,
                dst,
                dst_unresolved,
                kind: EdgeKind::Calls,
                confidence,
                file_id,
                order_key: r.node.start_byte() as u64,
            });
        }

        for (node, text) in imports {
            edges.push(Edge {
                src: file_pseudo_src.clone(),
                dst: None,
                dst_unresolved: Some(text),
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

impl SymbolExtractor for PythonExtractor {
    fn extract(&mut self, parsed: &ParsedSource) -> ParsedFile {
        let source = parsed.source().as_bytes();
        let root = parsed.ts_root();
        let file_id = parsed.file_id();
        let relative_path = parsed.relative_path();

        let mut parse_errors: Vec<ParseError> = Vec::new();
        if root.has_error() {
            parse_errors::collect(root, source, file_id, &mut parse_errors);
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

/// Python constructor filter is class-only: a same-file `class Foo:` shadowed
/// by `Foo(x)` is the canonical case. PascalCase fallback in
/// `is_constructor_call` covers external classes.
fn is_py_type_kind(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Struct)
}

fn build_symbol_match<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    pattern: usize,
    source: &[u8],
    file_id: FileId,
    relative_path: &str,
) -> Option<Symbol> {
    // pattern 0 → function_definition (may be method if enclosed in class)
    // pattern 1 → class_definition (mapped to SymbolKind::Struct, same shape
    // as the Rust extractor — class carries methods, like a struct does)
    let initial_kind = match pattern {
        0 => SymbolKind::Function,
        1 => SymbolKind::Struct,
        _ => return None,
    };
    let (name_node, def_node) = pick_name_and_def(m)?;
    let name = node_text(name_node, source);
    let kind = if matches!(initial_kind, SymbolKind::Function) && is_method(def_node) {
        SymbolKind::Method
    } else {
        initial_kind
    };
    let qualified_name = compute_qualified_name(def_node, &name, kind, source);
    let signature = compute_signature(def_node, source, kind);
    let anchor = node_anchor(def_node);
    let id = SymbolId::compute(relative_path, &qualified_name, kind, &signature);
    Some(Symbol {
        id,
        file_id,
        qualified_name,
        kind,
        signature,
        anchor,
    })
}

/// True when a `function_definition` lives inside a `class_definition`.
fn is_method(def_node: Node) -> bool {
    let mut cur = def_node.parent();
    while let Some(p) = cur {
        if p.kind() == "class_definition" {
            return true;
        }
        cur = p.parent();
    }
    false
}

/// Methods qualify as `Class.method`; module-level fns/classes stay bare.
fn compute_qualified_name(def_node: Node, name: &str, kind: SymbolKind, source: &[u8]) -> String {
    if matches!(kind, SymbolKind::Method)
        && let Some(class) = enclosing_class_name(def_node, source)
    {
        return format!("{class}.{name}");
    }
    name.to_string()
}

fn enclosing_class_name(def_node: Node, source: &[u8]) -> Option<String> {
    let mut cur = def_node.parent();
    while let Some(p) = cur {
        if p.kind() == "class_definition"
            && let Some(name_node) = p.child_by_field_name("name")
        {
            return Some(node_text(name_node, source));
        }
        cur = p.parent();
    }
    None
}

fn compute_signature(def_node: Node, source: &[u8], kind: SymbolKind) -> String {
    if !matches!(kind, SymbolKind::Function | SymbolKind::Method) {
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

/// Per-call reference threaded through `run_query`. Receiver semantics drive
/// Extracted-trust.
struct CallRef<'tree> {
    node: Node<'tree>,
    name: String,
    trust_intra_file: bool,
}

fn ref_call_match<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    source: &[u8],
) -> Option<CallRef<'tree>> {
    let node = m
        .captures
        .iter()
        .find(|c| c.node.kind() == "identifier")?
        .node;
    // Method calls (pattern 3 — `(call function: (attribute ...))`) trust the
    // intra-file map only when the receiver is `self` or `cls`.
    let trust_intra_file = if node.parent().is_some_and(|p| p.kind() == "attribute") {
        attribute_receiver_is_bare_self(node, source)
    } else {
        true
    };
    Some(CallRef {
        node,
        name: node_text(node, source),
        trust_intra_file,
    })
}

fn attribute_receiver_is_bare_self(method_ident: Node, source: &[u8]) -> bool {
    let Some(attr) = method_ident.parent().filter(|p| p.kind() == "attribute") else {
        return false;
    };
    let Some(recv) = attr.child_by_field_name("object") else {
        return false;
    };
    if recv.kind() != "identifier" {
        return false;
    }
    let text = node_text(recv, source);
    PYTHON_BARE_RECEIVERS.contains(&text.as_str())
}

fn pick_extracted_target<'a>(
    candidates: &[&'a Symbol],
    call_node: Node,
    caller: Option<&Symbol>,
) -> Option<&'a Symbol> {
    let is_method_call = call_node.parent().is_some_and(|p| p.kind() == "attribute");
    if is_method_call {
        pick_method_target(candidates, caller, DOT_SYNTAX.qname_sep)
    } else {
        unique(candidates.iter().copied().filter(|s| {
            matches!(
                s.kind,
                SymbolKind::Function | SymbolKind::Macro | SymbolKind::Const | SymbolKind::Static
            )
        }))
    }
}
