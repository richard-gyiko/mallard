# 0005 Ephemeral indexing for v0; defer true incremental indexing

## Status

Accepted

## Context

A long-running daemon with watch mode, AST patching, and incremental invalidation is the most general design but also the most expensive to build correctly. PR review does not actually need it: a PR is bounded by a base commit and a head commit. Indexing those two snapshots (and reusing the base across PRs in the same branch) is enough.

The temptation to build the "right" incremental engine first would burn months before any product validation.

## Decision

Stage the indexing engine:

- **v0** — full rebuild from a commit SHA. No persistence beyond a single index file per SHA.
- **v1** — cache base-commit indexes; reuse across PRs.
- **v2** — changed-file overlays on top of a cached base.
- **v3** — true incremental graph with file-watch and AST patching, **only if usage demands it**.

Each step is justified by observed pain in the previous step, not anticipation.

## Alternatives considered

### Build true incremental indexing first

Pros:
- Most general solution.
- Sub-second updates from the start.

Cons:
- Large engineering investment before any product feedback.
- AST patching and graph invalidation are subtle; bugs corrupt retrieval silently.
- Likely premature optimization for the PR-review wedge.

### No caching, always full rebuild

Pros:
- Simplest possible engine.

Cons:
- Unworkable beyond toy repos.
- Wastes work that PR review naturally has reason to cache (base commit).

### Staged path (v0 → v3)

Pros:
- Ships earliest.
- Each stage is justified by concrete pain.
- Caching is a small, safe step beyond full rebuild.

Cons:
- Some rework at each transition.
- May leave performance on the table for a window.

## Consequences

Positive:
- Time-to-first-product collapses.
- The hardest parts of indexing are deferred to when their value is proven.

Negative / tradeoffs:
- Large monorepos will be uncomfortable at v0. Acceptable scope.
- The eventual incremental engine is a known future investment, not avoided.

## Related

- `docs/specs/indexing/index-build.md`
- `docs/specs/indexing/index-query.md`
- `docs/decisions/0002-duckdb-as-graph-and-index-store.md`
- `docs/decisions/0006-pr-review-as-initial-wedge.md`
