# System

## Overview

Mallard is an AI-native repository index. It builds a persistent symbolic graph of a codebase and serves that graph to LLM-driven workflows. The first product surface is pull-request review.

Core thesis: LLMs are strong enough; the bottleneck is repository context retrieval. Mallard treats repository structure as durable knowledge — symbols, imports, callers, dependencies, tests, ownership — rather than transient embedding chunks. The LLM is a consumer of the index, not the center of the system.

## Modules

- **Parser** — Tree-sitter front-ends per supported language. Extracts symbols, imports, references, structural facts. See [decisions/0003-tree-sitter-and-ast-grep-parsing.md](decisions/0003-tree-sitter-and-ast-grep-parsing.md).
- **Index build** — Walks repo, drives parser, computes stable symbol IDs, writes graph to store. See [specs/indexing/index-build.md](specs/indexing/index-build.md).
- **Index query** — Read-only primitives over a built index: lookup, neighbors, bounded expansion, findings. See [specs/indexing/index-query.md](specs/indexing/index-query.md).
- **Store** — DuckDB-backed graph and symbol tables. Ordered edge tables, recursive CTEs. See [decisions/0002-duckdb-as-graph-and-index-store.md](decisions/0002-duckdb-as-graph-and-index-store.md).
- **Structural rules engine** — ast-grep rule runner for deterministic findings (anti-patterns, framework rules, lint-like signals).
- **Retrieval** — Symbolic-first, graph-aware retrieval over the index. Composes index-query primitives into ranked subgraphs. Embeddings are secondary. See [specs/retrieval/symbolic-graph-retrieval.md](specs/retrieval/symbolic-graph-retrieval.md) and [decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md](decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md).
- **PR reviewer** — Wedge product. Consumes retrieval + structural findings + LLM synthesis. See [specs/pr-review/pull-request-review.md](specs/pr-review/pull-request-review.md).

## Data flows

1. Repo snapshot (commit SHA) → parser → symbols + edges → store.
2. PR diff → changed-file overlay → retrieval (symbols touched, callers, blast radius) + structural findings → LLM reviewer → review output.

## Integrations

- **Git** — read-only repository access; commit SHA is the indexing unit.
- **GitHub / PR providers** — diff input + review output (target surface; not committed yet).
- **LLM provider** — review synthesis. Provider-agnostic at the boundary.

## Deployment

Local-first. Single Rust binary, DuckDB file as the index. No daemon, no watch mode in v0. See [decisions/0005-ephemeral-indexing-defer-incremental.md](decisions/0005-ephemeral-indexing-defer-incremental.md).

## Language

Rust. Justified by Tree-sitter / ast-grep / DuckDB ecosystem fit and the local-first indexing workload. See [decisions/0001-rust-as-implementation-language.md](decisions/0001-rust-as-implementation-language.md).

## Related specs

- [specs/indexing/index-build.md](specs/indexing/index-build.md)
- [specs/indexing/index-query.md](specs/indexing/index-query.md)
- [specs/retrieval/symbolic-graph-retrieval.md](specs/retrieval/symbolic-graph-retrieval.md)
- [specs/pr-review/pull-request-review.md](specs/pr-review/pull-request-review.md)

## Related decisions

- [decisions/0001-rust-as-implementation-language.md](decisions/0001-rust-as-implementation-language.md)
- [decisions/0002-duckdb-as-graph-and-index-store.md](decisions/0002-duckdb-as-graph-and-index-store.md)
- [decisions/0003-tree-sitter-and-ast-grep-parsing.md](decisions/0003-tree-sitter-and-ast-grep-parsing.md)
- [decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md](decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md)
- [decisions/0005-ephemeral-indexing-defer-incremental.md](decisions/0005-ephemeral-indexing-defer-incremental.md)
- [decisions/0006-pr-review-as-initial-wedge.md](decisions/0006-pr-review-as-initial-wedge.md)
