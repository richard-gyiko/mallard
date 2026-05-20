# 0006 PR review as the initial product wedge

## Status

Accepted

## Context

The underlying engine (symbolic graph index over a repository) could power many surfaces: chat-over-code, refactor planning, onboarding tours, security review, architecture audits. Choosing the wrong first surface stretches the engine across too many shapes of demand and slows validation.

PR review has unusually good properties as an initial wedge:

- **Bounded context** — a diff is a finite, well-defined input.
- **Diff-driven retrieval** — the changed symbols are the natural query anchor; aligns directly with [0004](0004-symbolic-graph-retrieval-over-embeddings-first.md).
- **Architectural reasoning matters** — blast radius, callers, affected tests are exactly what symbolic graph retrieval is good at.
- **Measurable output** — review comments can be compared to human reviewers, accepted/rejected, scored.
- **High business value** — every team does PR review; existing tools mostly do shallow lint.
- **Evaluation surface** — historical PRs are a ready-made evaluation set.

General "coding agent" lacks all of these — unbounded context, unclear success metric, hard to evaluate.

## Decision

PR review is the v0 product surface. Engine work is justified by what PR review needs, not by speculative future surfaces.

Other surfaces (chat-over-code, refactor planning, etc.) are explicitly **out of scope for v0**.

## Alternatives considered

### General coding agent

Pros:
- Larger total addressable surface.

Cons:
- Unbounded context budget.
- No clear evaluation.
- Crowded space; differentiation is hard.

### Architecture / onboarding tours

Pros:
- Showcases the graph nicely.
- Lower stakes than review.

Cons:
- Low repeat usage.
- Weak measurable outcome.
- Hard to monetize early.

### PR review

Pros / cons: see Context above.

## Consequences

Positive:
- Engine roadmap collapses to "what does PR review need next."
- Evaluation is straightforward (golden PR set, comment quality scoring).
- Sales / adoption story is concrete.

Negative / tradeoffs:
- Resisting scope creep into adjacent surfaces will be ongoing discipline.
- Some engine capabilities useful for other surfaces will be deferred even when "almost free."

## Related

- `docs/specs/pr-review/pull-request-review.md`
- `docs/decisions/0004-symbolic-graph-retrieval-over-embeddings-first.md`
- `docs/decisions/0005-ephemeral-indexing-defer-incremental.md`
