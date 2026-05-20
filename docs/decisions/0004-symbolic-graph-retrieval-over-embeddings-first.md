# 0004 Symbolic and graph retrieval first, embeddings second

## Status

Accepted

## Context

Naive chunk-based RAG underperforms on code tasks. The signal that matters for PR review — what calls this function, what does it touch transitively, which tests cover it, who owns it — is structural, not lexical-semantic. Embedding similarity finds "looks similar"; the graph finds "is connected."

The industry is converging on symbolic + graph-aware retrieval as the primary mechanism, with embeddings as a complementary fallback for fuzzy/natural-language queries.

## Decision

Retrieval primary path:

1. Resolve query to symbols / files / commits (symbolic anchors).
2. Expand via graph edges (callers, callees, imports, references, tests, ownership).
3. Rank by structural signal (distance, edge type, change recency, ownership relevance).

Embeddings are a secondary mechanism, invoked when symbolic anchors are missing or for natural-language exploration. Not the default path.

## Alternatives considered

### Embeddings-first (chunk RAG)

Pros:
- Simple to build.
- Handles natural-language queries out of the box.

Cons:
- Weak signal for "what does this change break."
- Loses structural relationships entirely.
- Already proven insufficient for code in current industry consensus.

### Graph-only, no embeddings

Pros:
- Fully deterministic retrieval.
- Simpler model.

Cons:
- Brittle when input is a natural-language question without a clear symbolic anchor.

### Hybrid, symbolic-first

Pros:
- Strong signal for the dominant code task (PR review).
- Falls back gracefully for fuzzy queries.
- Aligns with where the field is heading.

Cons:
- More moving parts than either pure approach.

## Consequences

Positive:
- Retrieval quality scales with index quality, not embedding-model upgrades.
- LLM context becomes a curated graph neighborhood, not a top-k chunk soup.

Negative / tradeoffs:
- Symbolic retrieval depends on parser quality; bad symbol extraction degrades the whole stack.
- Embedding infrastructure still needs to exist for the secondary path — not free.

## Related

- `docs/specs/retrieval/symbolic-graph-retrieval.md`
- `docs/specs/pr-review/pull-request-review.md`
- `docs/specs/indexing/index-query.md`
- `docs/decisions/0002-duckdb-as-graph-and-index-store.md`
- `docs/decisions/0003-tree-sitter-and-ast-grep-parsing.md`
