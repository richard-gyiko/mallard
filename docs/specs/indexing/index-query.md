# Index query

## Purpose

Serve read-only queries against a built index. Provides the primitive operations that retrieval, PR review, and any future surface compose into higher-level capabilities. Build-time concerns are out of scope (see [index-build.md](index-build.md)).

## Behavior

- The system must answer point lookups by symbol ID and return the symbol record (qualified name, kind, file, anchor, signature shape).
- The system must list direct neighbors of a symbol along requested edge kinds (callers, callees, importers, imports, contained-by, contains, tests-for, tested-by, references).
- The system must expand a symbol's neighborhood to a bounded depth, returning the subgraph (nodes + edges) traversed.
- The system must list structural-rule findings filtered by path prefix, rule ID, or symbol ID.
- The system must answer file/module-level queries: importers of a file, symbols defined in a file, files owning a path.
- The system must surface the index `metadata` (SHA, indexer version, rule-set hash) on demand.
- The system must tolerate the index file being absent or atomically replaced; failed queries must not corrupt caller state.

## Rules

- Queries are read-only. The query surface must not mutate the index, even for cache or counter side effects.
- An index file is queried as a single immutable snapshot. Queries do not span SHAs.
- Bounded depth is mandatory for neighborhood expansion. Unbounded traversal must be rejected with an explicit error, not silently truncated.
- Cycles in traversal must be detected and broken; results must not be infinite.
- Returned anchors (file path + line range) reflect the SHA the index was built from. The caller is responsible for mapping anchors to a different SHA if needed.
- Edge kinds and finding rule IDs are part of the public contract. Renaming or repurposing them is a breaking change.

## Inputs and outputs

Inputs:

- Path to a built index file (DuckDB).
- Query type + parameters: symbol ID(s), edge-kind filter, depth bound, path prefix, rule ID filter.

Outputs:

- Symbol records, edge records, subgraph (nodes + edges), finding records, file/module records.
- Index metadata on request.
- Empty result sets (not errors) when a query matches nothing.

## Edge cases

- Symbol ID not in index — empty result, not an error.
- Depth = 0 — returns the source symbol only, no edges.
- Path prefix matches no files — empty result.
- Index file present but built by an incompatible indexer version — explicit error referencing the version mismatch.
- Concurrent reads while a new index is being written elsewhere — readers must see a consistent snapshot (the old file or the atomically replaced new file), never a partial state.

## Observability

- Per-query timing.
- Counter per query type.
- Hot-symbol counter (most-requested symbol IDs) for caching opportunities.
- Slow-query log above a configurable threshold.

## Related

- `docs/system.md`
- `docs/specs/indexing/index-build.md`
- `docs/specs/retrieval/symbolic-graph-retrieval.md`
- `docs/specs/pr-review/pull-request-review.md`
- `docs/decisions/0002-duckdb-as-graph-and-index-store.md`
- `docs/decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md`
