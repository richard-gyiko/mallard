# Symbolic graph retrieval

> **Status: deferred.** Per [ADR-0007](../../decisions/0007-defer-retrieval-module-agents-compose-primitives.md), v0 retrieval is delivered by agents composing `mallard query` primitives via an Agent Skill rather than by a dedicated built module. This spec remains as the eventual shape if a future in-process Rust consumer needs the same capabilities behind a single function. [ADR-0004](../../decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md) (symbolic-first over embeddings-first) still stands.

## Purpose

Resolve a query into a ranked, bounded subgraph of repository context that an LLM (or any other consumer) can reason over. Retrieval composes index-query primitives into structural neighborhoods that are useful for downstream tasks like PR review. Embeddings are a secondary mechanism, used only when symbolic anchors are absent.

## Behavior

- The system must accept one of: a symbol ID, a set of symbol IDs, a file path, a diff (changed symbols), or a natural-language query.
- The system must resolve the input to one or more symbolic anchors before expansion. If no anchors resolve and natural-language input is given, the system may fall back to embedding similarity over symbol-level documents.
- The system must expand each anchor via graph edges (callers, callees, importers, imports, tests-for, tested-by, references) using bounded depth and bounded fan-out.
- The system must rank the resulting subgraph by structural signal (edge kind, hop distance, ownership relevance, change recency where available).
- The system must apply a per-query context budget (node count, edge count, or token estimate) and drop lowest-ranked items first.
- The system must return the ranked subgraph along with the ranking rationale per included item (which signals contributed).
- The system must label every retrieved item with its source path (symbolic vs embedding).

## Rules

- Symbolic retrieval is the default path. Embedding fallback is invoked only when no symbolic anchor resolves, or when explicitly requested.
- Retrieval is deterministic for deterministic inputs along the symbolic path. The ranking function must be reproducible.
- Retrieval must not hallucinate: every returned symbol, edge, and finding must come from the underlying index. Synthesizing context is out of scope here.
- The retrieval surface must not depend on PR review or any other consumer's data model. It returns generic subgraphs, not review-shaped output.
- Bounded depth and bounded fan-out are mandatory. Unbounded retrieval must be rejected with an explicit error.
- Embedding-fallback results must never be returned without a symbolic anchor when one exists.

## Inputs and outputs

Inputs:

- Anchor: symbol ID(s), file path, diff (changed-symbol set), or natural-language query.
- Configuration: depth bound, fan-out bound, edge-kind allow-list, context budget, ranking-weight overrides.
- Reference to a built index (see [index-query.md](../indexing/index-query.md)).

Outputs:

- Ranked subgraph: ordered list of (node, score, rationale, source: `symbolic | embedding`).
- Edge set connecting returned nodes.
- Query metadata: anchors resolved, fallback path taken, items dropped due to budget.

## Edge cases

- No anchors resolve and embedding fallback disabled — empty result with explicit `no-anchor` reason.
- Single anchor with no neighbors — return the anchor alone.
- Anchor in a parse-failed file — return the file record with `unparseable` marker; do not silently omit.
- Diff input where every changed symbol is new (no base-side analog) — proceed; expansion uses head-side edges only.
- Context budget smaller than the resolved anchors themselves — return anchors only, drop all expansion.
- Index missing or version-mismatched — propagate the index-query error; do not invent fallback results.

## Observability

- Per-query: anchors-resolved count, embedding-fallback used (bool), nodes/edges before vs after budget, ranking-weight profile used, timing.
- Counter per anchor type (symbol, file, diff, NL).
- Distribution of dropped-item counts (signals budget-pressure trends).
- Hit-rate on symbolic resolution (how often NL queries resolve without fallback).

## Related

- `docs/system.md`
- `docs/specs/indexing/index-query.md`
- `docs/specs/pr-review/pull-request-review.md`
- `docs/decisions/0002-duckdb-as-graph-and-index-store.md`
- `docs/decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md`
