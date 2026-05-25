use tree_sitter::Query;

use crate::core::{ParseError, ParsedFile, Result};
use crate::extractor::SymbolExtractor;
use crate::parse_errors;
use crate::parsed_source::ParsedSource;

/// Python `SymbolExtractor`. Scaffolding-only at A1: indexes Python files
/// without crashing, produces empty symbol/edge lists. Symbol patterns and
/// call extraction land in A2/A3. See `docs/plans/move-1-python-ts-action.md`.
pub struct PythonExtractor {
    // Constructing the Query at startup keeps the language link verified at
    // build time; the (empty) query itself isn't run until A2 fills it in.
    #[allow(dead_code)]
    query: Query,
}

impl PythonExtractor {
    pub fn new() -> Result<Self> {
        let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
        let query = Query::new(&language, "")?;
        Ok(PythonExtractor { query })
    }
}

impl SymbolExtractor for PythonExtractor {
    fn extract(&mut self, parsed: &ParsedSource) -> ParsedFile {
        let file_id = parsed.file_id();
        let root = parsed.ts_root();
        let mut parse_errors: Vec<ParseError> = Vec::new();
        if root.has_error() {
            parse_errors::collect(root, parsed.source().as_bytes(), file_id, &mut parse_errors);
        }
        ParsedFile {
            file_id,
            symbols: Vec::new(),
            edges: Vec::new(),
            parse_errors,
            parse_ms: parsed.parse_ms,
            query_ms: 0,
        }
    }
}
