# System

## Overview

Mallard is an AI-native repository index. It builds a persistent symbolic graph of a codebase and serves that graph to LLM-driven workflows. The first product surface is pull-request review.

Core thesis: LLMs are strong enough; the bottleneck is repository context retrieval. Mallard treats repository structure as durable knowledge — symbols, imports, callers, dependencies, tests, ownership — rather than transient embedding chunks. The LLM is a consumer of the index, not the center of the system.

## Modules

- **Parser / SymbolExtractor** — Per-language adapter behind `SymbolExtractor` trait. Rust today; tree-sitter front-end, extracts symbols, edges, parse errors. See [decisions/0003-tree-sitter-and-ast-grep-parsing.md](decisions/0003-tree-sitter-and-ast-grep-parsing.md).
- **Parsed source** — One tree-sitter parse per file, shared between symbol extraction and rule matching.
- **File processor** — Per-file pipeline. Holds the `ParsedSource`, dispatches the language-appropriate `SymbolExtractor` and rule matcher, records timing.
- **Index build** — Walks repo, drives the file processor, computes stable symbol IDs, writes graph to store, runs post-build name resolution. See [specs/indexing/index-build.md](specs/indexing/index-build.md) and [decisions/0008-heuristic-name-resolution.md](decisions/0008-heuristic-name-resolution.md).
- **Index reader** — Verified handle to a built index. Read-only primitives: lookup, neighbors, bounded expansion, findings, file/module queries, metadata. See [specs/indexing/index-query.md](specs/indexing/index-query.md).
- **Store** — DuckDB-backed graph and symbol tables. Ordered edge tables, recursive CTEs. See [decisions/0002-duckdb-as-graph-and-index-store.md](decisions/0002-duckdb-as-graph-and-index-store.md).
- **Structural rules engine** — ast-grep rule runner for deterministic findings (anti-patterns, framework rules, lint-like signals).
- **Retrieval** — Agent-composed via the CLI primitives + Agent Skill in v0; no dedicated built module yet. Symbolic-first stays the policy ([decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md](decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md)); delivery shape per [decisions/0007-defer-retrieval-module-agents-compose-primitives.md](decisions/0007-defer-retrieval-module-agents-compose-primitives.md). Eventual built-module shape sketched in [specs/retrieval/symbolic-graph-retrieval.md](specs/retrieval/symbolic-graph-retrieval.md).
- **PR reviewer** — Wedge product. Built as an agent flow that calls `mallard query` primitives, not as a Rust consumer of a built retrieval module. See [specs/pr-review/pull-request-review.md](specs/pr-review/pull-request-review.md).

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
