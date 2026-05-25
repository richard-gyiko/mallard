# Pull-request review

> **Note on LLM-synthesis layer:** This spec was written before [ADR-0013](../../decisions/0013-kill-phase-d-pivot-agent-verification.md) killed the LLM-synthesis layer. Mallard's permanent product shape is deterministic-only. References below to "LLM synthesis" and "synthesized findings" describe the historical layered-pipeline framing; the shipped product implements only the deterministic stages (3, 4, 5, 7). Agent-facing surfaces ship as plain CLI primitives via the Anthropic Agent Skill at `skills/mallard/SKILL.md`. See [ADR-0013](../../decisions/0013-kill-phase-d-pivot-agent-verification.md) for the rationale.
>
> **Architecture pattern (historical framing):** see [ADR-0009](../../decisions/0009-pr-review-architecture-pattern.md). The wedge is a layered pipeline (selective retrieval → deterministic analyzers → LLM synthesis → severity-calibrated output → project memory), framed as a *first-pass assistant with human oversight*, not a merge gate. Retrieval is composed by the agent via `mallard query` primitives per [ADR-0007](../../decisions/0007-defer-retrieval-module-agents-compose-primitives.md).

## Purpose

Produce an architectural review of a pull request: comments grounded in what the diff touches, what it depends on, what it likely breaks, and which structural rules it violates. Comments are evidence-backed by the repository graph, not free-form LLM commentary over a diff.

Review composes two engine capabilities: index queries ([index-query.md](../indexing/index-query.md)) and retrieval ([symbolic-graph-retrieval.md](../retrieval/symbolic-graph-retrieval.md)). It does not duplicate their concerns.

## Behavior

- The system must accept a base commit SHA, a head commit SHA, and a repository reference.
- The system must ensure an index exists for both commits ([index-build.md](../indexing/index-build.md)). Re-using cached indexes is preferred.
- The system must compute the diff at symbol granularity: which symbols were added, removed, modified, or renamed between the two indexes.
- The system must call retrieval with the changed-symbol set as input and consume the returned ranked subgraph plus structural findings as the evidence pool.
- The system must emit review comments anchored to specific file/line positions in the head commit's diff.
- Every emitted comment must cite the structural evidence it depends on (symbol IDs, edge paths, finding IDs). Comments without citations must not be emitted.
- The system must distinguish between deterministic findings (ast-grep rule violations) and synthesized findings (LLM reasoning over retrieved subgraph), and label each comment accordingly.
- The system must respect a per-PR comment budget. Lower-confidence comments are dropped before higher-confidence ones.

## Rules

- The review must not invent symbols, files, or call relationships that are not present in the index. Hallucinated references are a correctness failure, not a style issue.
- The review must operate read-only on the repository. It must not write to the working tree, push to remotes, or modify any index file.
- Deterministic findings (structural-rule layer) must produce identical output for identical (base SHA, head SHA, rule set). LLM-synthesized comments may vary, but the retrieval input they reason over must be reproducible.
- Files outside the diff may be referenced as evidence but must not be the target of a comment.
- The reviewer must not re-implement retrieval, ranking, or graph traversal logic. Cross-cutting concerns belong in their owning spec.

## Inputs and outputs

Inputs:

- Repository reference, base SHA, head SHA.
- Optional: PR title/description (soft signal, not authoritative).
- Optional: reviewer focus areas (e.g., security, performance) — bias retrieval ranking weights, not gate output.
- Optional: comment budget, structural rule allow-list.

Outputs:

- Ordered list of review comments. Each comment carries:
  - file path + line range in head commit
  - body
  - source kind: `structural-rule | graph-synthesis`
  - confidence score
  - cited evidence (symbol IDs, edge paths, rule IDs)
- Review summary: counts by source kind, by severity, total tokens used, indexer cache hit/miss, retrieval-budget drops, comment-budget drops.

## Edge cases

- Diff touches no parseable code (docs-only, config-only) — emit an empty review with a summary marker; do not synthesize commentary.
- Renames detected by Git but not by the indexer — fall back to "removed + added" symbol diff; flag as `low-confidence rename`.
- Base or head SHA missing from the repository — error out; do not silently choose a nearby commit.
- Very large diffs (above configurable thresholds) — produce a partial review covering top-ranked changed symbols and flag the truncation explicitly.
- Symbol present in both base and head with same ID but different signature — treat as modified; do not treat as remove + add.
- Generated files in the diff — surface but do not comment unless a rule explicitly targets them.
- Retrieval returns empty subgraph for a changed symbol — emit at most a deterministic-finding comment, no synthesis.

## Observability

- Per-review record: base SHA, head SHA, indexer cache status, retrieval timings, LLM tokens, comment counts by kind/severity, dropped-comment counts (retrieval budget vs comment budget).
- Per-comment audit trail: prompt, retrieved evidence subset, model, output.
- Evaluation hook: comments tagged with stable IDs so accept/reject feedback can be attributed back to retrieval and synthesis paths separately.

## Related

- `docs/system.md`
- `docs/specs/indexing/index-build.md`
- `docs/specs/indexing/index-query.md`
- `docs/specs/retrieval/symbolic-graph-retrieval.md`
- `docs/decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md`
- `docs/decisions/0005-ephemeral-indexing-defer-incremental.md`
- `docs/decisions/0006-pr-review-as-initial-wedge.md`
