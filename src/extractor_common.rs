//! Shared per-language extractor utilities.
//!
//! The Rust, Python, and TypeScript extractors share a common pipeline shape
//! (collect symbols → bucket by short name → emit Contains/Calls/Imports
//! edges with confidence tiering). The pieces that don't vary by language
//! live here; per-language behaviour is parameterised through a
//! [`LangSyntax`] descriptor plus closures where richer dispatch is needed.
//!
//! The split is intentionally minimal — only invariants and obvious
//! parameterisations are hoisted. Rust's macro-body walking, Python's
//! `self`/`cls` receiver check, and TypeScript's two-grammar dispatch stay
//! local to their respective extractors.
use std::collections::HashMap;

use tree_sitter::Node;

use crate::core::{Anchor, Symbol, SymbolKind};

/// Per-language syntax knobs used by the shared helpers. The qualified-name
/// separator is the only piece of grammar that leaks into pure-string logic
/// (`Rust::foo`, `Python.foo`, `TypeScript.foo`).
#[derive(Copy, Clone)]
pub struct LangSyntax {
    pub qname_sep: &'static str,
}

pub const RUST_SYNTAX: LangSyntax = LangSyntax { qname_sep: "::" };
pub const DOT_SYNTAX: LangSyntax = LangSyntax { qname_sep: "." };

/// Byte slice → owned `String`, lossy on invalid UTF-8.
pub fn node_text(node: Node, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned()
}

pub fn node_anchor(node: Node) -> Anchor {
    Anchor {
        start_byte: node.start_byte() as u64,
        end_byte: node.end_byte() as u64,
        start_line: node.start_position().row as u32,
        end_line: node.end_position().row as u32,
    }
}

/// Return the sole item from `it` if it yields exactly one, else None.
pub fn unique<T, I: Iterator<Item = T>>(mut it: I) -> Option<T> {
    let first = it.next()?;
    if it.next().is_some() {
        None
    } else {
        Some(first)
    }
}

/// Normalise a parameter-list source slice: trim, strip outer parens,
/// collapse internal whitespace runs, drop a trailing comma. Used for
/// signature stability across formatting variants.
pub fn canonical_params(text: String) -> String {
    let trimmed = text.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed);
    let normalized = inner.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = normalized.trim_end_matches(',').trim();
    format!("({normalized})")
}

/// The trailing segment of a qualified name (after the last separator), or
/// `None` for bare names.
pub fn short_name<'a>(qualified: &'a str, sep: &str) -> Option<&'a str> {
    qualified.rsplit_once(sep).map(|(_, last)| last)
}

/// The leading segment of a qualified name (the `Type` in `Type::method`),
/// or `None` for bare names.
pub fn impl_type_prefix<'a>(qualified: &'a str, sep: &str) -> Option<&'a str> {
    qualified.rsplit_once(sep).map(|(prefix, _)| prefix)
}

/// Smallest capture = name identifier, largest = enclosing def_node.
pub fn pick_name_and_def<'tree>(
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

/// Smallest symbol whose anchor strictly contains `node`'s start byte.
pub fn find_enclosing_definition<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let start = node.start_byte() as u64;
    symbols
        .iter()
        .filter(|s| s.anchor.start_byte <= start && start < s.anchor.end_byte)
        .min_by_key(|s| s.anchor.end_byte - s.anchor.start_byte)
}

/// Bucket symbols by their short (post-last-separator) name. Borrowing
/// references keep this cheap — callers walk the result without re-hashing.
pub fn symbols_by_short<'a>(symbols: &'a [Symbol], sep: &str) -> HashMap<&'a str, Vec<&'a Symbol>> {
    let mut map: HashMap<&'a str, Vec<&'a Symbol>> = HashMap::with_capacity(symbols.len());
    for s in symbols {
        let key = short_name(&s.qualified_name, sep).unwrap_or(s.qualified_name.as_str());
        map.entry(key).or_default().push(s);
    }
    map
}

/// Disambiguate intra-file short-name candidates for a method-position call:
/// require the candidate to share the caller's impl-type prefix, then dedupe
/// by qualified name so inherent+trait pairs (or re-extracted duplicates)
/// collapse to a single target.
pub fn pick_method_target<'a>(
    candidates: &[&'a Symbol],
    caller: Option<&Symbol>,
    sep: &str,
) -> Option<&'a Symbol> {
    let caller_prefix = caller.and_then(|s| impl_type_prefix(&s.qualified_name, sep))?;
    let matching: Vec<&Symbol> = candidates
        .iter()
        .copied()
        .filter(|s| impl_type_prefix(&s.qualified_name, sep) == Some(caller_prefix))
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
}

/// Shared `Foo(x)` → constructor filter. `name` is the call's captured
/// identifier text; `type_kinds` is the set of language-specific SymbolKinds
/// that, when found under `name`, mark the call as a type construction (Rust:
/// Struct/Enum/Trait/TypeAlias; Python: Struct; TS: Struct/Trait/TypeAlias).
///
/// The PascalCase fallback matches identifiers whose last segment starts
/// uppercase and contains no `_`: those are very likely external-class
/// constructors (`requests.Session(...)` captures `Session`). We only veto
/// when no callable (Function/Method/Macro) under that short name is defined
/// in this file — otherwise a same-file `PascalCaseFn()` would be lost.
pub fn is_constructor_call(
    name: &str,
    symbols_by_short: &HashMap<&str, Vec<&Symbol>>,
    sep: &str,
    stdlib_variants: &[&str],
    is_type_kind: fn(SymbolKind) -> bool,
) -> bool {
    if stdlib_variants.contains(&name) {
        return true;
    }
    let candidates = symbols_by_short.get(name).map(Vec::as_slice).unwrap_or(&[]);
    if candidates.iter().any(|s| is_type_kind(s.kind)) {
        return true;
    }
    let last_segment = name.rsplit(sep).next().unwrap_or(name);
    let pascal_like = last_segment
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        && !last_segment.contains('_');
    if pascal_like {
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
