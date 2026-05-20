# 0002 DuckDB as graph and index store

## Status

Accepted

## Context

The store must hold symbols, edges (imports, calls, references, contains), ownership, test linkage, and the diff overlays used by PR review. Workload is mostly analytical and join-heavy: traverse callers of a changed symbol, walk reverse dependencies, intersect ownership with blast radius. Writes are batched per index build, not transactional per request. Deployment is local-first.

A dedicated graph database (Neo4j, etc.) was the obvious first thought. It is not the right fit for a local-first analytical engine at this stage.

## Decision

Use DuckDB as the graph and index store. Model edges as ordered tables; traverse with recursive CTEs.

## Alternatives considered

### Neo4j or other native graph DB

Pros:
- Native graph traversal semantics.
- Mature query language (Cypher) for graph patterns.

Cons:
- Adds a daemon to a local-first product.
- Heavier deployment, packaging, and ops story.
- Analytical joins (counts, aggregates over neighborhoods) are weaker than columnar engines.
- Overkill for graph sizes that fit in memory at v0.

### SQLite

Pros:
- Trivial embedding.
- Universally available.

Cons:
- Row-oriented; analytical queries slower at scale.
- Weaker recursive CTE / window function ergonomics for graph workloads.
- No first-class columnar storage.

### Custom in-memory graph

Pros:
- Maximum control.

Cons:
- Reinvents persistence, query layer, and tooling.
- Loses SQL as a debugging surface.

### DuckDB

Pros:
- Columnar, fast for join-heavy analytical traversal.
- Embedded, single-file — fits local-first deployment.
- Strong recursive CTE support.
- Append/batch indexing fits the rebuild workflow.
- SQL is a debugging and exploration surface from day one.
- Good Rust client.

Cons:
- Not a native graph engine; deep multi-hop traversals require careful query design.
- Concurrent writers limited (acceptable — indexer is the sole writer).

## Consequences

Positive:
- No extra daemon. The index is a file.
- Standard SQL toolchain for inspection.
- Headroom for analytical queries (ownership × blast radius × test coverage) without re-platforming.

Negative / tradeoffs:
- Deeper graph algorithms (PageRank, community detection) will need either DuckDB extensions or out-of-store computation.
- If write concurrency or live updates become a hard requirement, the store choice will need revisiting.

## Related

- `docs/specs/indexing/index-build.md`
- `docs/specs/indexing/index-query.md`
- `docs/decisions/0005-ephemeral-indexing-defer-incremental.md`
- `docs/system.md`
