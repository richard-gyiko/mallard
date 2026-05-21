# 0008 Heuristic name resolution; defer stack-graphs and language-server integration

## Status

Accepted

## Context

The Rust extractor builds a per-file `HashMap<short_name, Symbol>` and resolves call-site references against it. Cross-file calls land as `dst_unresolved`. A post-build pass in `IndexWriter::resolve_edges` then runs a second resolution against the full symbol table, picking the unambiguous callable (Function / Method / Macro) match for each unresolved name.

This is *heuristic* name resolution. It is correct often enough to make the wedge useful but it cannot match name resolution that participates in the language's actual scoping rules. Two industrial alternatives exist:

- **stack-graphs** (GitHub). Per-language TSG rules encode scope semantics; resolution becomes a path-finding search. Powers GitHub's Precise Code Navigation. **Repository archived 2025-09-09**; GitHub no longer maintains it. Forking and maintaining it is a non-trivial commitment for a project our size, with no upstream upstream.
- **LSP / language-server protocol** (rust-analyzer, gopls, pyright, etc.). Authoritative resolution; per-language servers. Breaks [0003](0003-tree-sitter-and-ast-grep-parsing.md) (single tree-sitter front-end across many languages) and introduces heavyweight per-language daemons with slow startup.

Dogfooded against this repo, the heuristic resolver settles:

- 100% of intra-file calls (parser side; trivial).
- ~10% of cross-file calls (resolver side; the remainder target stdlib or external crates, unsolvable without dependency graph knowledge).
- A constructor filter in the extractor drops tuple-struct / enum-variant call noise so the remaining unresolved set is mostly real external calls, not parser confusion.

The unresolved tail is dominated by stdlib (`format!`, `println!`, `Vec::new`, `HashMap::new`, etc.) and third-party crates. Resolving those requires modelling Cargo's dependency graph and the stdlib's qualified-name space — work that is itself a project, not a refinement.

## Decision

Keep heuristic name resolution. Improve it incrementally when call-site evidence shows specific gaps:

1. **Intra-file**: per-file `HashMap<short_name, Symbol>` (extractor-side).
2. **Cross-file**: post-build resolver that picks unambiguous callable matches against the global symbol table.
3. **Constructor filter** (Rust-specific, behind the `SymbolExtractor` seam): drop calls whose name resolves to a same-file `Struct` / `Enum` / `Trait` / `TypeAlias`, is a stdlib variant constructor (`Ok` / `Err` / `Some` / `None`), or is PascalCase with no known same-file callable.

Stdlib / external resolution is out of scope until the PR-review wedge surfaces a concrete failure case where it matters.

stack-graphs is **not adopted** — archived upstream, maintenance burden too high.

LSP / language-server integration is **not adopted** — breaks ADR-0003, per-language daemon model is the wrong shape for a single Rust binary running ephemeral builds.

## Alternatives considered

### stack-graphs

Pros:
- Battle-tested in GitHub's Precise Code Navigation.
- Per-language scope rules are explicit (TSG files).
- File-incremental resolution semantics align with our future incremental-indexing plans ([0005](0005-ephemeral-indexing-defer-incremental.md)).

Cons:
- **Repository archived 2025-09-09.** No upstream maintenance.
- Adopting it means forking + maintaining the resolver crate ourselves.
- Per-language TSG rules are a substantial authoring burden (and they are *also* unmaintained upstream now).
- Excessive precision for the wedge today; gains nothing on stdlib / external calls without an additional dependency-graph piece.

### LSP / language-server protocol

Pros:
- Authoritative resolution per language.
- Reuses the tooling each language community already maintains.

Cons:
- Breaks [0003](0003-tree-sitter-and-ast-grep-parsing.md): we'd swap a single tree-sitter front-end for a per-language daemon zoo.
- Slow startup and large memory footprint per language server.
- Network of `initialize` / `workspace/didChangeWatchedFiles` lifecycle messages — wrong shape for ephemeral builds.
- Multi-language indexing becomes operationally painful.

### Heuristic (this decision)

Pros:
- Already shipped; resolver passes dogfood smoke tests.
- Composes with the existing extractor seam: per-language constructor filtering lives inside each `SymbolExtractor` implementation.
- No external service or language-specific scope-rule files to maintain.

Cons:
- Cannot resolve cross-crate or stdlib targets without modelling Cargo / package graphs.
- "Unambiguous callable match" can miss real ambiguity (two same-named functions in different files both target the call). Acceptable; ambiguous edges stay unresolved, not silently wrong.
- Per-language extractors must each implement their own constructor / variant filter when added; not free.

## Consequences

Positive:
- The resolver is one Rust function we own; no external dependency to track.
- Failures are visible (`dst_unresolved` survives in the graph); consumers can decide what to do.
- Adding a second language extractor only requires its own intra-file resolver + constructor filter — the post-build resolver works generically across languages.

Negative / tradeoffs:
- Cross-crate calls stay unresolved indefinitely. PR-review prompts must tolerate seeing "calls `format!`" without a target symbol.
- If stack-graphs is ever revived (community fork, or GitHub re-opens it), revisit this ADR.
- Heuristic resolution will sometimes mis-resolve in the presence of identically-named symbols across files. Current implementation refuses ambiguous matches; tightening means more false negatives, looser means more false positives. The "unambiguous callable" rule errs toward false negatives, which is the correct bias for evidence-driven review.

## Related

- [0003-tree-sitter-and-ast-grep-parsing.md](0003-tree-sitter-and-ast-grep-parsing.md) — single tree-sitter front-end; LSP integration would violate it.
- [0005-ephemeral-indexing-defer-incremental.md](0005-ephemeral-indexing-defer-incremental.md) — ephemeral builds; per-language daemons don't fit.
- [docs/specs/indexing/index-build.md](../specs/indexing/index-build.md) — symbol extraction + resolver are part of build.
- [docs/specs/indexing/index-query.md](../specs/indexing/index-query.md) — consumers see resolved or unresolved edges via `IndexReader`.
