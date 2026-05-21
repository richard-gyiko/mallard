# 0007 Defer dedicated retrieval module; agents compose primitives via CLI + Agent Skill

## Status

Accepted

## Context

[ADR-0004](0004-symbolic-graph-retrieval-over-embeddings-first.md) chose symbolic-first retrieval over embeddings-first, and [docs/specs/retrieval/symbolic-graph-retrieval.md](../specs/retrieval/symbolic-graph-retrieval.md) sketched a dedicated retrieval module: anchor resolution, multi-anchor BFS, structural ranking, per-query budget, embedding fallback. ADR-0004 still stands. This ADR is about *how the retrieval shape is delivered*, not about whether retrieval is symbolic.

Three developments since ADR-0004 changed the cost / benefit of building a dedicated module:

- **Attention dilution is empirical, not folklore.** SWE-PRBench (2026) found that *every* one of eight frontier models *degraded* when prompts moved from structured diff-only context to richer file- and repo-context, attributing the loss to attention dilution rather than missing information. The takeaway is "retrieve and structure the smallest decisive context," not "give the LLM more." See [docs/research/agentic-code-review-tools.md](../research/agentic-code-review-tools.md).
- **Code execution with MCP** (Anthropic, 2025): agents that compose tool primitives in their own execution environment dominate agents that consume pre-composed monolithic tools. The benchmark cited (Google Drive → Salesforce: 150k → 2k tokens) is a ~99% context reduction by letting the agent filter and combine results in code rather than receiving pre-ranked outputs.
- **Industry hybrids** (Sourcegraph Cody, modern Cursor): production retrieval stacks are heavy — keyword + embeddings + code graph + RRF reranking + intent classifier. They serve broad surfaces (autocomplete, chat-over-code, refactoring). Mallard's wedge ([0006](0006-pr-review-as-initial-wedge.md)) is narrower: anchor = changed-symbol set, structural neighborhood matters, NL queries are rare.

Combined, these say: a built retrieval module that pre-composes a single "right" subgraph + ranking is the *wrong* shape for a wedge whose ceiling is already capped by attention dilution. The right shape is small primitives + an agent that retrieves the *minimum* needed.

The `IndexReader` already exposes the read primitives an agent needs to compose retrieval itself: `lookup_symbol`, `neighbors`, `expand`, `findings`, `symbols_in_file`, `importers_of_file`, `files_at_prefix`, `metadata`. These ship in the CLI as `mallard query <verb>` subcommands. An Agent Skill (markdown describing the verbs, output shape, and idiomatic compositions) is enough to teach an LLM to assemble PR-review context one verb at a time.

The dedicated retrieval module would mainly add: server-side ranking, budget enforcement, and embedding fallback. Each of those is currently speculative for the wedge:

- **Ranking**: hop-distance + edge-kind is the floor. Beyond that, modern LLMs cope with mild ordering noise once a bounded subgraph is in hand. Pre-composing one ranking removes the agent's option to compose differently.
- **Budget**: each `mallard query expand` already takes a depth bound. Multi-anchor budget enforcement is a Bash chain (`head -n`, `jq slice`) until evidence shows otherwise.
- **Embedding fallback**: the wedge never lacks a symbolic anchor (the diff *is* the anchor set). Fallback solves a problem PR review doesn't have.

## Decision

Defer the dedicated retrieval module. v0 retrieval = `IndexReader` library API + `mallard query` CLI + Agent Skill that teaches composition patterns.

The PR-review wedge will be built as an agent flow that calls `mallard query` primitives, not as a Rust consumer of a built `retrieval::retrieve(...)`. The library API stays available for any future in-process Rust caller that wants the same composition.

Delivery surfaces in priority order:

1. **Library API** — `IndexReader` (shipped). For in-process Rust callers.
2. **CLI + Agent Skill** — `mallard query` already exists; Skill comes next. Distributed via git, no install dance, composes with `grep`, `jq`, `gh`, `sed` in the same Bash tool every agent has.
3. **MCP server** — deferred until a non-Claude-Code consumer (Cursor, Cline, Continue) shows up, or until persistent state (incremental indexing, hot cache) makes a per-call process spawn untenable.

## Alternatives considered

### Ship the spec's full retrieval module now

Pros:
- Honors the spec literally.
- Single in-process API for any consumer.

Cons:
- ~500 LoC against a use case (anchor=diff) that's already well-served by the existing primitives.
- Bakes in ranking + budget decisions before any consumer has driven them.
- Closes off the agent-composition route.

### Retrieval-lite (multi-anchor BFS + hop-distance ranking + budget, ~150 LoC)

Pros:
- Spec-aligned, smaller surface than the full module.
- Single function for PR review and other Rust consumers.

Cons:
- Still pre-composes one ranking the agent could have picked itself.
- Doesn't unlock the CLI + Skill composition surface — needs separate work.
- For one user (the wedge), the indirection earns less than the agent-composition route.

### MCP server first

Pros:
- Universal reach (Cursor, Cline, Continue, VS Code Copilot, ChatGPT).
- Persistent connection; one DuckDB open across calls; lower per-call latency.

Cons:
- New code (Rust MCP SDK or Node wrapper, days, not minutes).
- Server lifecycle to install and run per IDE.
- Today's user is Claude Code, which speaks Skill natively.
- Tools siloed in MCP namespace; agent loses composition with `grep` / `jq` / `gh` in one Bash chain.

### CLI + Agent Skill (this decision)

Pros:
- Zero new code on the CLI side; Skill is ~30 lines of markdown.
- Composes with every other shell tool the agent already has.
- Works today against this very repo (dogfood-ready).
- Single distribution path (git clone the skill).

Cons:
- Reach limited to agents that speak the Skill format (Claude Code today; ecosystem is expanding).
- Per-call process spawn + DuckDB open (~250ms) — fine at v0 scale, would matter on huge monorepos with deep expand.
- No persistent state (caching, watch mode) without going through the library API directly.

## Consequences

Positive:
- Engine roadmap stays narrow: ship Skill, dogfood, build PR-review flow on top.
- Ranking + budget decisions defer to actual usage data, not speculation.
- Library API and CLI stay the only public surfaces; both are already mature.
- Migration to a built retrieval module later is non-breaking — it composes the same primitives the agent would.

Negative / tradeoffs:
- Non-Claude-Code agents can't use mallard until the MCP server lands. Acceptable while the wedge is being validated; revisit when a real cross-IDE user appears.
- Each CLI call pays process-spawn + DuckDB-open latency. Mitigated by keeping per-PR-review tool-call counts in the 5–15 range; MCP becomes a perf optimisation when that ceiling is hit.
- The retrieval spec ([docs/specs/retrieval/symbolic-graph-retrieval.md](../specs/retrieval/symbolic-graph-retrieval.md)) is now "spec for the eventual module, not the v0 shape". Marked as deferred there.
- Symbol-diff is similarly deferred — agents can compose it via `mallard query symbols-in-file` against two indexes when the PR-review flow needs it. A dedicated diff primitive lands when call-site evidence shows the composition is awkward.

## Related

- [0004-symbolic-graph-retrieval-over-embeddings-first.md](0004-symbolic-graph-retrieval-over-embeddings-first.md) — stands; this ADR refines *how* the symbolic-first shape is delivered.
- [0006-pr-review-as-initial-wedge.md](0006-pr-review-as-initial-wedge.md) — the wedge that defines what retrieval needs to serve.
- [0009-pr-review-architecture-pattern.md](0009-pr-review-architecture-pattern.md) — the layered-pipeline pattern the agent composes into.
- [docs/specs/retrieval/symbolic-graph-retrieval.md](../specs/retrieval/symbolic-graph-retrieval.md) — marked deferred; spec remains as the eventual built-module shape.
- [docs/specs/pr-review/pull-request-review.md](../specs/pr-review/pull-request-review.md) — wedge spec; the agent-flow caller.
- [docs/specs/indexing/index-query.md](../specs/indexing/index-query.md) — the primitives the agent composes.
- [docs/research/agentic-code-review-tools.md](../research/agentic-code-review-tools.md) — primary source for attention-dilution + agentic-pipeline arguments.
