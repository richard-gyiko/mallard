# System

## Overview

Mallard is an AI-native repository index. It builds a persistent symbolic graph of a codebase and serves that graph to LLM-driven workflows. The first product surface is pull-request review.

Core thesis: LLMs are strong enough; the bottleneck is repository context retrieval. Mallard treats repository structure as durable knowledge — symbols, imports, callers, dependencies, tests, ownership — rather than transient embedding chunks. The LLM is a consumer of the index, not the center of the system.

## Modules

- **Parser / SymbolExtractor** — Per-language adapter behind `SymbolExtractor` trait. **Rust, Python, TypeScript / TSX** today; tree-sitter front-end, extracts symbols, edges, parse errors. Per-language quirks isolated in `src/extractor_<lang>.rs`; cross-language invariants live in `src/extractor_common.rs`. See [decisions/0003-tree-sitter-and-ast-grep-parsing.md](decisions/0003-tree-sitter-and-ast-grep-parsing.md) and [decisions/0012-multi-language-extractor-architecture.md](decisions/0012-multi-language-extractor-architecture.md).
- **Parsed source** — One tree-sitter parse per file, shared between symbol extraction and rule matching.
- **File processor** — Per-file pipeline. Holds the `ParsedSource`, dispatches the language-appropriate `SymbolExtractor` and rule matcher, records timing.
- **Index build** — Walks repo, drives the file processor, computes stable symbol IDs, writes graph to store, runs post-build name resolution. See [specs/indexing/index-build.md](specs/indexing/index-build.md) and [decisions/0008-heuristic-name-resolution.md](decisions/0008-heuristic-name-resolution.md).
- **Index reader** — Verified handle to a built index. Read-only primitives: lookup, neighbors, bounded expansion, findings, file/module queries, metadata. See [specs/indexing/index-query.md](specs/indexing/index-query.md).
- **Store** — DuckDB-backed graph and symbol tables. Ordered edge tables, recursive CTEs. See [decisions/0002-duckdb-as-graph-and-index-store.md](decisions/0002-duckdb-as-graph-and-index-store.md).
- **Structural rules engine** — ast-grep rule runner for deterministic findings (anti-patterns, framework rules, lint-like signals).
- **Retrieval** — Agent-composed via the CLI primitives + Agent Skill in v0; no dedicated built module yet. Symbolic-first stays the policy ([decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md](decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md)); delivery shape per [decisions/0007-defer-retrieval-module-agents-compose-primitives.md](decisions/0007-defer-retrieval-module-agents-compose-primitives.md). Eventual built-module shape sketched in [specs/retrieval/symbolic-graph-retrieval.md](specs/retrieval/symbolic-graph-retrieval.md).
- **PR reviewer** — Wedge product. Two delivery shapes:
  - **`mallard pr-review` subcommand** (v1, shipped) — deterministic-only stages 3–5; no LLM call. Consumed by the `mallard-review` composite GitHub Action under `.github/actions/review/`. See [decisions/0011-deterministic-only-pr-review-v1.md](decisions/0011-deterministic-only-pr-review-v1.md) and [decisions/0013-kill-phase-d-pivot-agent-verification.md](decisions/0013-kill-phase-d-pivot-agent-verification.md).
  - **Agent skill** — Anthropic Agent Skills format manifest at `skills/mallard/SKILL.md`, distributed via [skills.sh](https://www.skills.sh). Agents (Claude Code, Codex CLI, ChatGPT) shell-exec the four agent-facing CLI primitives (`find` / `blast-radius` / `test-seams` / `symbol-diff`). Deterministic output only — no LLM call from mallard's surface.

## Data flows

1. Repo snapshot (commit SHA) → parser → symbols + edges → store.
2. PR diff → changed-file overlay → retrieval (symbols touched, callers, blast radius) + structural findings → review output.
3. Agent query (skill-invoked) → CLI primitive (`find` / `blast-radius` / `test-seams` / `symbol-diff`) → JSON on stdout with `schema_version: "1.0"`.

## Integrations

- **Git** — read-only repository access; commit SHA is the indexing unit.
- **GitHub PR provider** — diff input via `git diff --name-only`; review output via `gh pr comment`. Composite Action at `.github/actions/review/action.yml`.
- **Agent skill** — mallard ships as an [Anthropic Agent Skill](https://github.com/anthropics/skills) at `skills/mallard/SKILL.md`. Distributed via [skills.sh](https://www.skills.sh). Reaches Claude Code, Codex CLI, ChatGPT; other agents shell-exec the CLI directly.
- **LLM provider** — none. Deterministic-only is the permanent product shape per [ADR-0011](decisions/0011-deterministic-only-pr-review-v1.md) and [ADR-0013](decisions/0013-kill-phase-d-pivot-agent-verification.md).

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
- [decisions/0007-defer-retrieval-module-agents-compose-primitives.md](decisions/0007-defer-retrieval-module-agents-compose-primitives.md)
- [decisions/0008-heuristic-name-resolution.md](decisions/0008-heuristic-name-resolution.md)
- [decisions/0009-pr-review-architecture-pattern.md](decisions/0009-pr-review-architecture-pattern.md)
- [decisions/0010-edge-confidence-tier.md](decisions/0010-edge-confidence-tier.md)
- [decisions/0011-deterministic-only-pr-review-v1.md](decisions/0011-deterministic-only-pr-review-v1.md)
- [decisions/0012-multi-language-extractor-architecture.md](decisions/0012-multi-language-extractor-architecture.md)
