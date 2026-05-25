use ast_grep_language::SupportLang;
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

/// Shared symbol + reference query — patterns are valid for both the
/// TypeScript and TSX grammars (TSX is a superset that only adds JSX
/// nodes; our captures don't touch those).
const TS_QUERY: &str = r#"
(function_declaration name: (identifier) @def.function.name) @def.function

(class_declaration name: (type_identifier) @def.class.name) @def.class

(interface_declaration name: (type_identifier) @def.interface.name) @def.interface

(type_alias_declaration name: (type_identifier) @def.type_alias.name) @def.type_alias

(method_definition name: (property_identifier) @def.method.name) @def.method

(call_expression function: (identifier) @ref.call.simple) @ref.call

(call_expression function: (member_expression property: (property_identifier) @ref.call.method)) @ref.call

(import_statement) @import.decl
"#;

/// TypeScript / TSX `SymbolExtractor`. Holds two pre-compiled Query
/// instances since `tree_sitter::Query` is bound to a specific Language;
/// the TSX grammar is identical to TS for the patterns we extract but
/// loaded under a different language pointer.
pub struct TypeScriptExtractor {
    query_ts: Query,
    query_tsx: Query,
    cursor: QueryCursor,
}

impl TypeScriptExtractor {
    pub fn new() -> Result<Self> {
        let lang_ts: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let lang_tsx: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();
        let query_ts = Query::new(&lang_ts, TS_QUERY)?;
        let query_tsx = Query::new(&lang_tsx, TS_QUERY)?;
        Ok(TypeScriptExtractor {
            query_ts,
            query_tsx,
            cursor: QueryCursor::new(),
        })
    }

    fn run_query(
        &mut self,
        root: Node<'_>,
        source: &[u8],
        file_id: FileId,
        relative_path: &str,
        language: SupportLang,
    ) -> (Vec<Symbol>, Vec<Edge>) {
        let mut symbols: Vec<Symbol> = Vec::new();
        let mut references: Vec<CallRef> = Vec::new();
        let mut imports: Vec<(Node, String)> = Vec::new();

        let query = if matches!(language, SupportLang::Tsx) {
            &self.query_tsx
        } else {
            &self.query_ts
        };
        let mut matches = self.cursor.matches(query, root, source);
        while let Some(m) = matches.next() {
            match m.pattern_index {
                0..=4 => {
                    if let Some(sym) =
                        build_symbol_match(m, m.pattern_index, source, file_id, relative_path)
                    {
                        symbols.push(sym);
                    }
                }
                5 | 6 => {
                    if let Some(r) = ref_call_match(m, source) {
                        references.push(r);
                    }
                }
                7 => {
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

        for r in references {
            if is_constructor_call(&r.name, &by_short, DOT_SYNTAX.qname_sep, &[], is_ts_type_kind) {
                continue;
            }
            let enclosing_sym = find_enclosing_definition(r.node, &symbols);
            let enclosing = enclosing_sym
                .map(|s| s.id.clone())
                .unwrap_or_else(|| file_pseudo_src.clone());
            let dst = if r.trust_intra_file {
                let candidates = by_short.get(r.name.as_str()).map(Vec::as_slice).unwrap_or(&[]);
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

impl SymbolExtractor for TypeScriptExtractor {
    fn extract(&mut self, parsed: &ParsedSource) -> ParsedFile {
        let source = parsed.source().as_bytes();
        let root = parsed.ts_root();
        let file_id = parsed.file_id();
        let relative_path = parsed.relative_path();
        let language = parsed.language();

        let mut parse_errors: Vec<ParseError> = Vec::new();
        if root.has_error() {
            parse_errors::collect(root, source, file_id, &mut parse_errors);
        }

        let t_query = std::time::Instant::now();
        let (symbols, edges) = self.run_query(root, source, file_id, relative_path, language);
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

struct CallRef<'tree> {
    node: Node<'tree>,
    name: String,
    trust_intra_file: bool,
}

/// Type-kind set treated as a constructor under same-file capture:
/// classes (Struct), interfaces (Trait), and type aliases.
fn is_ts_type_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Struct | SymbolKind::Trait | SymbolKind::TypeAlias
    )
}

fn build_symbol_match<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    pattern: usize,
    source: &[u8],
    file_id: FileId,
    relative_path: &str,
) -> Option<Symbol> {
    // 0 → function_declaration, 1 → class, 2 → interface, 3 → type_alias,
    // 4 → method_definition. Interfaces map to SymbolKind::Trait (closest
    // semantic match; both define a contract that other types implement).
    let initial_kind = match pattern {
        0 => SymbolKind::Function,
        1 => SymbolKind::Struct,
        2 => SymbolKind::Trait,
        3 => SymbolKind::TypeAlias,
        4 => SymbolKind::Method,
        _ => return None,
    };
    let (name_node, def_node) = pick_name_and_def(m)?;
    let name = node_text(name_node, source);
    let qualified_name = compute_qualified_name(def_node, &name, initial_kind, source);
    let signature = compute_signature(def_node, source, initial_kind);
    let anchor = node_anchor(def_node);
    let id = SymbolId::compute(relative_path, &qualified_name, initial_kind, &signature);
    Some(Symbol {
        id,
        file_id,
        qualified_name,
        kind: initial_kind,
        signature,
        anchor,
    })
}

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
        if p.kind() == "class_declaration"
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
        if child.kind() == "formal_parameters" {
            return canonical_params(node_text(child, source));
        }
    }
    String::new()
}

fn ref_call_match<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    source: &[u8],
) -> Option<CallRef<'tree>> {
    let name_capture = m
        .captures
        .iter()
        .find(|c| matches!(c.node.kind(), "identifier" | "property_identifier"))?;
    let node = name_capture.node;
    // Method calls (`(member_expression property: ...)`) trust the intra-file
    // map only when the receiver is bare `this`. Anything else
    // (`this.field.foo()`, `obj.foo()`) emits Unresolved so the post-build
    // resolver tiers — same shape as Rust Gap 2 + Python self/cls.
    let trust_intra_file = if node.kind() == "property_identifier" {
        member_receiver_is_bare_this(node)
    } else {
        true
    };
    Some(CallRef {
        node,
        name: node_text(node, source),
        trust_intra_file,
    })
}

fn member_receiver_is_bare_this(prop_ident: Node) -> bool {
    let Some(member) = prop_ident.parent().filter(|p| p.kind() == "member_expression") else {
        return false;
    };
    let Some(obj) = member.child_by_field_name("object") else {
        return false;
    };
    obj.kind() == "this"
}

fn pick_extracted_target<'a>(
    candidates: &[&'a Symbol],
    call_node: Node,
    caller: Option<&Symbol>,
) -> Option<&'a Symbol> {
    if call_node.kind() == "property_identifier" {
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
