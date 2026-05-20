use std::path::Path;

use tree_sitter::{Node, Parser as TsParser, Query, QueryCursor, StreamingIterator, Tree};

use crate::core::{
    Anchor, Edge, EdgeKind, FileId, MallardError, ParseError, Result, Symbol, SymbolId, SymbolKind,
};

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

pub struct ParsedFile {
    pub file_id: FileId,
    pub source: Vec<u8>,
    pub tree: Tree,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    pub parse_errors: Vec<ParseError>,
    pub parse_ms: u64,
    pub query_ms: u64,
}

pub struct RustParser {
    parser: TsParser,
    query: Query,
}

impl RustParser {
    pub fn new() -> Result<Self> {
        let mut parser = TsParser::new();
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language)?;
        let query = Query::new(&language, RUST_QUERY)?;
        Ok(RustParser { parser, query })
    }

    pub fn parse_file(
        &mut self,
        file_id: FileId,
        relative_path: &str,
        source: Vec<u8>,
    ) -> Result<ParsedFile> {
        let t_parse = std::time::Instant::now();
        let tree = self
            .parser
            .parse(&source, None)
            .ok_or_else(|| MallardError::Other(format!("parser returned None for {relative_path}")))?;
        let parse_ms = t_parse.elapsed().as_millis() as u64;

        let mut parse_errors: Vec<ParseError> = Vec::new();
        if tree.root_node().has_error() {
            collect_errors(tree.root_node(), &source, file_id, &mut parse_errors);
        }

        let t_query = std::time::Instant::now();
        let (symbols, edges) = self.extract(&tree, &source, file_id, relative_path)?;
        let query_ms = t_query.elapsed().as_millis() as u64;

        Ok(ParsedFile {
            file_id,
            source,
            tree,
            symbols,
            edges,
            parse_errors,
            parse_ms,
            query_ms,
        })
    }

    fn extract(
        &self,
        tree: &Tree,
        source: &[u8],
        file_id: FileId,
        relative_path: &str,
    ) -> Result<(Vec<Symbol>, Vec<Edge>)> {
        let mut cursor = QueryCursor::new();
        let mut symbols: Vec<Symbol> = Vec::new();
        let mut references: Vec<(Node, String, EdgeKind)> = Vec::new();
        let mut imports: Vec<(Node, String)> = Vec::new();

        let mut matches = cursor.matches(&self.query, tree.root_node(), source);
        while let Some(m) = matches.next() {
            let pattern = m.pattern_index;
            match pattern {
                0..=8 => {
                    if let Some(sym) = build_symbol_match(m, pattern, source, file_id, relative_path) {
                        symbols.push(sym);
                    }
                }
                9 | 10 | 11 => {
                    if let Some((node, name)) = ref_call_match(m, source) {
                        references.push((node, name, EdgeKind::Calls));
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

        let symbols_by_name: std::collections::HashMap<String, &Symbol> = symbols
            .iter()
            .map(|s| (s.qualified_name.clone(), s))
            .collect();

        let file_pseudo_src = SymbolId(format!("file:{}", relative_path));
        for sym in &symbols {
            edges.push(Edge {
                src: file_pseudo_src.clone(),
                dst: Some(sym.id.clone()),
                dst_unresolved: None,
                kind: EdgeKind::Contains,
                file_id,
                order_key: sym.anchor.start_byte,
            });
        }

        for (node, name, kind) in references {
            let enclosing = find_enclosing_definition(node, &symbols)
                .map(|s| s.id.clone())
                .unwrap_or_else(|| file_pseudo_src.clone());
            let dst = symbols_by_name.get(&name).map(|s| s.id.clone());
            let dst_unresolved = if dst.is_none() { Some(name.clone()) } else { None };
            edges.push(Edge {
                src: enclosing,
                dst,
                dst_unresolved,
                kind,
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
                file_id,
                order_key: node.start_byte() as u64,
            });
        }

        symbols.sort_by_key(|s| s.anchor.start_byte);
        edges.sort_by_key(|e| (e.kind.as_str(), e.order_key));

        Ok((symbols, edges))
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
    let name_node = m.captures[0].node;
    let def_node = m.captures[1].node;
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

fn ref_call_match<'tree>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    source: &[u8],
) -> Option<(Node<'tree>, String)> {
    let name_capture = m.captures.iter().rev().find(|c| {
        let kind = c.node.kind();
        kind == "identifier" || kind == "field_identifier"
    })?;
    Some((name_capture.node, node_text(name_capture.node, source)))
}

fn compute_qualified_name(def_node: Node, name: &str, kind: SymbolKind, source: &[u8]) -> String {
    if matches!(kind, SymbolKind::Function) {
        if let Some(impl_type) = enclosing_impl_type(def_node, source) {
            return format!("{impl_type}::{name}");
        }
    }
    name.to_string()
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

fn canonical_params(text: String) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
            let mut walker = p.walk();
            for child in p.children(&mut walker) {
                if child.kind() == "type_identifier"
                    || child.kind() == "generic_type"
                    || child.kind() == "scoped_type_identifier"
                {
                    return Some(node_text(child, source));
                }
            }
            return None;
        }
        if p.kind() == "trait_item" {
            let mut walker = p.walk();
            for child in p.children(&mut walker) {
                if child.kind() == "type_identifier" {
                    return Some(node_text(child, source));
                }
            }
            return None;
        }
        cur = p.parent();
    }
    None
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

#[allow(dead_code)]
pub fn _validate(_p: &Path) -> Result<()> {
    Ok(())
}
