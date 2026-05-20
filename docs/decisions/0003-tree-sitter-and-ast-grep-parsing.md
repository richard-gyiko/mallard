# 0003 Tree-sitter and ast-grep for parsing and structural findings

## Status

Accepted

## Context

Indexing needs to extract symbols, imports, references, and structural patterns across many languages without requiring a compiler frontend per language. PR review specifically needs:

- changed symbols
- nearby architecture
- affected dependencies
- structural patterns
- likely blast radius

Perfect semantic correctness (type resolution, generics, dynamic dispatch, macros) is not required at v0. The product can extract significant value from syntactic + structural signal.

## Decision

Use **Tree-sitter** as the parsing front-end for all supported languages. Use **ast-grep** as the structural rule engine for deterministic findings.

Defer compiler-frontend / LSP-grade semantic enrichment until product validation justifies the cost. See [0005](0005-ephemeral-indexing-defer-incremental.md) for the staging philosophy.

## Alternatives considered

### Language-specific compilers / LSPs

Pros:
- True type resolution, alias resolution, generics, macro expansion.
- Highest semantic fidelity.

Cons:
- One integration per language.
- Heavy runtime cost (spawn compilers, manage workspaces).
- Disproportionate effort for v0 value.

### Tree-sitter only (no ast-grep)

Pros:
- One dependency.

Cons:
- Reinventing a rule layer on raw ASTs.
- Loses ast-grep's framework-aware rule library.

### ast-grep only

Pros:
- Structural rules out of the box.

Cons:
- Need lower-level AST and symbol extraction; Tree-sitter is the canonical source ast-grep itself builds on.

### Tree-sitter + ast-grep

Pros:
- Tree-sitter handles parsing across many languages with one integration model.
- Per-language symbol extraction follows the established `queries/tags.scm` convention shipped by most grammars, plus custom S-expression queries with `@captures` for project-specific needs.
- ast-grep adds AST pattern matching and framework-aware rules without compiler frontends.
- Both are Rust-native; integrates cleanly with [0001](0001-rust-as-implementation-language.md).
- Deterministic findings complement LLM explanation cleanly.

Cons:
- Syntactic-only — misses semantics behind aliases, generics, dynamic dispatch.
- Per-language symbol-extraction queries are still bespoke beyond what `tags.scm` covers; grammars that ship without `tags.scm` need queries written from scratch.

## Consequences

Positive:
- One parsing pipeline across languages.
- Deterministic structural findings shippable from day one.
- Headroom: ast-grep rules can grow without re-architecting.

Negative / tradeoffs:
- Semantic gaps will surface (especially in dynamic and macro-heavy code). Acknowledged and accepted for v0; revisit per [0005](0005-ephemeral-indexing-defer-incremental.md).
- Each new language requires its own symbol-extraction queries.

## Related

- `docs/specs/indexing/index-build.md`
- `docs/decisions/0001-rust-as-implementation-language.md`
- `docs/decisions/0005-ephemeral-indexing-defer-incremental.md`
