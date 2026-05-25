use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use crate::core::{
    Anchor, Edge, EdgeConfidence, EdgeKind, FileId, ParseError, ParsedFile, Result, Symbol,
    SymbolId, SymbolKind,
};
use crate::extractor::SymbolExtractor;
use crate::parse_errors;
use crate::parsed_source::ParsedSource;

const PYTHON_QUERY: &str = r#"
(function_definition name: (identifier) @def.function.name) @def.function

(class_definition name: (identifier) @def.class.name) @def.class
"#;

/// Python `SymbolExtractor`. A2 ships definitions (functions, methods,
/// classes); call extraction + Gap-2/3 ports land in A3. Mirrors the Rust
/// extractor's shape — see [`crate::extractor::RustExtractor`].
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

        let mut matches = self.cursor.matches(&self.query, root, source);
        while let Some(m) = matches.next() {
            let pattern = m.pattern_index;
            if let Some(sym) = build_symbol_match(m, pattern, source, file_id, relative_path) {
                symbols.push(sym);
            }
        }

        let file_pseudo_src = SymbolId(format!("file:{}", relative_path));
        let mut edges: Vec<Edge> = Vec::with_capacity(symbols.len());
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

/// Smallest capture = name identifier, largest = enclosing def_node.
fn pick_name_and_def<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
) -> Option<(Node<'tree>, Node<'tree>)> {
    let mut smallest: Option<Node> = None;
    let mut largest: Option<Node> = None;
    for c in m.captures {
        let n = c.node;
        let span = n.end_byte().saturating_sub(n.start_byte());
        if smallest.is_none_or(|s| span < s.end_byte().saturating_sub(s.start_byte())) {
            smallest = Some(n);
        }
        if largest.is_none_or(|l| span > l.end_byte().saturating_sub(l.start_byte())) {
            largest = Some(n);
        }
    }
    Some((smallest?, largest?))
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
/// Nested classes resolve as `Outer.Inner`; nested fn definitions don't add
/// a path component (treated as siblings of the enclosing class for now).
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
