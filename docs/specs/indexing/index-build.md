# Index build

## Purpose

Construct a persistent symbolic graph of a repository at a specific commit SHA. Build is a one-way producer: it walks the working tree, parses with Tree-sitter, extracts symbols / edges / structural findings, and writes the resulting graph to the index store. Query-time concerns are out of scope (see [index-query.md](index-query.md)).

## Behavior

- The system must accept a repository path and a commit SHA and produce an index keyed by that SHA.
- The system must extract, per source file: defined symbols, imported symbols, references, containment (file → module → symbol), and structural facts emitted by ast-grep rules.
- The system must assign every symbol a stable identity (qualified name + kind + signature shape + file path) that survives whitespace, formatting, and unrelated edits in the same file.
- The system must persist the graph in a DuckDB file co-located with the index.
- The system must treat each index build as immutable for the SHA it was built from. Re-indexing the same SHA must be either a no-op (cache hit) or produce an identical result.
- The system must be the sole writer to a given index file during a build.

## Rules

- A symbol's stable ID is content-addressable from (file path, qualified name, kind, signature shape). It must not embed line numbers, commit SHA, or build timestamp.
- Languages without a Tree-sitter grammar are out of scope. Unknown extensions are skipped, not errored.
- Per-language symbol extraction uses each grammar's `queries/tags.scm` when available; custom S-expression queries fill gaps.
- Parse failures on a file degrade gracefully: the file is recorded as `unparseable` with the error, but indexing of other files continues.
- The build must not store source contents beyond what is needed for retrieval (snippets, anchors). Full-file mirroring is forbidden.
- The build is deterministic for a given (repo SHA, indexer version, rule-set hash). Non-determinism is a defect.

## Inputs and outputs

Inputs:

- Repository working tree (or Git object database) at a specific commit SHA.
- Optional ast-grep rule set.
- Optional language allow-list.
- Optional per-file size threshold.

Outputs:

- DuckDB index file containing: `symbols`, `edges`, `files`, `parse_errors`, `findings`, `metadata` (SHA, indexer version, rule-set hash).
- Build summary: file counts, symbol counts, edge counts per type, parse-error count, elapsed time.

## Edge cases

- Empty repository — produces a valid, empty index, not an error.
- Submodules / vendored code — indexed if inside the working tree; not followed across submodule boundaries by default.
- Generated files — indexed if present on disk; the system does not run generators.
- Symlinks — followed only if they resolve inside the working tree; cycles detected and broken.
- Binary files — skipped silently.
- Very large files (above threshold) — recorded with a `skipped: size` marker rather than parsed.
- Same qualified name in two paths — produces two distinct symbol IDs (file path is part of identity).

## Observability

- Build summary written to stdout/JSON on completion.
- Per-file timing for the slowest N files.
- `parse_errors` table queryable for triage.
- Indexer version + rule-set hash stamped in `metadata` for reproducibility checks.
- Counters: files indexed, symbols extracted, edges extracted, findings emitted, files skipped (by reason).

## Related

- `docs/system.md`
- `docs/specs/indexing/index-query.md`
- `docs/specs/retrieval/symbolic-graph-retrieval.md`
- `docs/decisions/0002-duckdb-as-graph-and-index-store.md`
- `docs/decisions/0003-tree-sitter-and-ast-grep-parsing.md`
- `docs/decisions/0005-ephemeral-indexing-defer-incremental.md`
