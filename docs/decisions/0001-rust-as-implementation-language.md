# 0001 Rust as implementation language

## Status

Accepted

## Context

Mallard's durable value is an indexing engine: walk a repository, parse with Tree-sitter, extract symbols and edges, write to DuckDB, run ast-grep rules, serve graph queries. That workload is CPU-bound, I/O-heavy, and long-lived per invocation. The product ships as a local-first binary.

Python was initially attractive (faster iteration on prompts, ML ecosystem) but the prompts are not where the durable value lives. The index is.

## Decision

Implement Mallard in Rust.

## Alternatives considered

### Python

Pros:
- Fast prototyping.
- Rich ML / LLM client ecosystem.
- Tree-sitter, DuckDB, ast-grep all have Python bindings.

Cons:
- Indexing is CPU-bound; GIL and interpreter overhead hurt at repo scale.
- Local-first single-binary distribution is awkward.
- Type-system weaker for a long-lived graph engine.

### TypeScript / Node

Pros:
- Strong ecosystem if a web UI ships later.
- Tree-sitter bindings exist.

Cons:
- Indexing performance.
- DuckDB and ast-grep integration less first-class.

### Rust

Pros:
- Native Tree-sitter integration is first-class.
- ast-grep is Rust-native.
- DuckDB has a strong Rust client.
- Single static binary fits local-first deployment.
- Performance fits indexing and graph traversal workloads.

Cons:
- Slower iteration speed than Python for exploratory work.
- Smaller LLM-client ecosystem (manageable — provider HTTP APIs are simple).

## Consequences

Positive:
- One language for the whole engine, from parser to retrieval.
- Distributable as a single binary.
- Performance headroom for larger repos without re-architecting.

Negative / tradeoffs:
- LLM-side experimentation is slower than it would be in Python.
- Recruiting / contributor surface is narrower.

## Related

- `docs/system.md`
- `docs/decisions/0002-duckdb-as-graph-and-index-store.md`
- `docs/decisions/0003-tree-sitter-and-ast-grep-parsing.md`
