# Mallard

Domain language for the AI-native repository index. Architecture vocabulary lives in `.claude/skills/improve-codebase-architecture/LANGUAGE.md`; this file names the *things* mallard works on.

## Language

**Index**:
The DuckDB file produced by one build run, keyed by a commit SHA. Immutable for that SHA. Holds files, symbols, edges, findings, parse errors, and metadata.
_Avoid_: database, db, store (when referring to the file).

**IndexReader**:
A verified handle to a built **Index**. Opens once, checks `index_format_version`, then serves read primitives (lookup, neighbors, expand, findings, metadata) on `&self`. The interface is the test surface for query behaviour.
_Avoid_: client, accessor, repo.

**IndexWriter**:
The single writer to a build-time **Index**. Owns the temp-file-then-rename rename strategy so readers never see a torn snapshot.
_Avoid_: builder, inserter.

**Symbol**:
A definition extracted from a source file — function, method, struct, enum, trait, module, const, etc. Carries a content-addressed `SymbolId` derived from `(file path, qualified name, kind, signature)`. Stable across whitespace and formatting edits.
_Avoid_: definition, declaration, identifier (those are language-level concepts; `Symbol` is mallard's persisted record).

**Edge**:
A directed relation between symbols. Kinds: `calls`, `imports`, `contains`, `tests_for`, `tested_by`. `dst` may be unresolved (a name we couldn't bind to a symbol).
_Avoid_: link, relation, ref.

**Anchor**:
A byte + line range identifying a span in a source file at the indexed SHA. Used to point a caller (an LLM, a reviewer) at a precise location.
_Avoid_: position, location, range.

**Finding**:
The output of a structural rule applied to a file. Carries `rule_id`, file, line range, message. Deterministic for `(SHA, rule-set hash, indexer version)`.
_Avoid_: lint, violation, warning (those carry severity baggage mallard doesn't enforce yet).

**ParsedSource**:
One tree-sitter parse of a file, held in memory for the duration of a per-file pass. Shared by symbol extraction and rule matching so the same bytes aren't reparsed. Carries its file_id and relative_path so the **SymbolExtractor** trait takes a single argument.
_Avoid_: AST, tree, parsed file.

**FileProcessor**:
The per-file pipeline. Holds a **ParsedSource**, dispatches the language-appropriate **SymbolExtractor** + rule matcher, records timing and parse errors. The dispatch seam for languages lives behind its interface.
_Avoid_: pipeline, handler, processor (the bare word).

**SymbolExtractor**:
Per-language adapter. Turns a **ParsedSource** into the `ParsedFile` for that file — **Symbol**s, **Edge**s, parse errors. One impl per supported language (today: `RustExtractor`); **FileProcessor** picks the right one by `ParsedSource::language()`.
_Avoid_: parser, visitor, extractor (the bare word).

**QueryRequest** / **QueryResult**:
The request/response shape crossing the query seam. CLI marshals argv into a `QueryRequest`; future adapters (MCP, HTTP) do the same. Hidden from callers: SQL, row mapping, connection lifecycle.
_Avoid_: command, query (the bare word).

**index format version**:
Integer stamped in the **Index** metadata at build time. Readers verify a match before opening; mismatch is an explicit error, never silent fallback.
_Avoid_: schema version, db version.

## Example dialogue

> **Dev:** I want to find every symbol that calls `bump`.
> **Domain:** Open an `IndexReader`, then `.neighbors(bump_id, [Calls], In)`. The result is a list of `NeighborEdge`s — each one's `src` is a `Symbol` with an `Anchor` you can point an LLM at.
> **Dev:** And if `bump` was deleted between SHAs?
> **Domain:** The `Index` is immutable per SHA, so you open the head **Index** and the base **Index** separately. There's no "current" — every read is anchored to one SHA.
> **Dev:** Where does the `format!` warning in `greet.rs` come from — parser or rules?
> **Domain:** Rules. `parser.rs` produces **Symbol**s and **Edge**s; structural rules produce **Finding**s. Both run inside the **FileProcessor** off the same **ParsedSource**.
