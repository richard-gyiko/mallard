use ast_grep_core::AstGrep;
use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_language::{LanguageExt, SupportLang};
use tree_sitter::Node;

use crate::core::Result;

/// Single tree-sitter parse of one file, held for the duration of a per-file pass.
/// Both symbol extraction and rule matching consume the same parse — see CONTEXT.md.
pub struct ParsedSource {
    ast: AstGrep<StrDoc<SupportLang>>,
    language: SupportLang,
    pub parse_ms: u64,
}

impl ParsedSource {
    pub fn parse(language: SupportLang, source: &str) -> Result<Self> {
        let t = std::time::Instant::now();
        let ast = language.ast_grep(source);
        let parse_ms = t.elapsed().as_millis() as u64;
        Ok(ParsedSource { ast, language, parse_ms })
    }

    pub fn language(&self) -> SupportLang {
        self.language
    }

    pub fn source(&self) -> &str {
        self.ast.source()
    }

    pub fn ast(&self) -> &AstGrep<StrDoc<SupportLang>> {
        &self.ast
    }

    /// Borrow the underlying tree-sitter root node. Symbol extractors query against
    /// this; the same node lives behind ast-grep's pattern matching.
    pub fn ts_root(&self) -> Node<'_> {
        self.ast.root().get_inner_node()
    }
}
