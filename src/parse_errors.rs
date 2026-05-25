use tree_sitter::Node;

use crate::core::{FileId, ParseError};

/// Tree-sitter `ERROR` / `MISSING` nodes recursively flattened into
/// `ParseError`s. Shared across language extractors so the wire-level shape of
/// parser errors stays uniform regardless of grammar.
pub fn collect(node: Node, source: &[u8], file_id: FileId, out: &mut Vec<ParseError>) {
    if node.is_error() || node.is_missing() {
        out.push(ParseError {
            file_id,
            message: if node.is_missing() {
                format!("missing node: {}", node.kind())
            } else {
                let snippet = String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]);
                let short: String = snippet.chars().take(64).collect();
                format!("syntax error near: {short}")
            },
            line: node.start_position().row as u32,
            col: node.start_position().column as u32,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.has_error() || child.is_error() || child.is_missing() {
            collect(child, source, file_id, out);
        }
    }
}
