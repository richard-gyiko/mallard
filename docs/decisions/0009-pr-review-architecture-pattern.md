# 0009 PR review as a layered pipeline, not a chatbot with repo access

## Status

Accepted

## Context

[ADR-0006](0006-pr-review-as-initial-wedge.md) chose PR review as the v0 product surface. That decision said *what* to build first; it did not say *how* to build the reviewer. The 2026 literature on agentic code review is now strong enough to commit to an architectural pattern before we write the wedge.

The synthesis in [docs/research/agentic-code-review-tools.md](../research/agentic-code-review-tools.md) is unusually crisp on five points:

1. **Reliable reviewers are pipelines, not chatbots.** IRIS, SAILOR, QLCoder, Sonar, Sourcegraph, GitHub, OpenAI, and Anthropic have converged on a layered pattern: selective retrieval → deterministic analyzers → LLM synthesis → patch / claim validation → severity-calibrated output → project memory. "LLM on a diff" loses to this pattern across every benchmark that measures something other than text similarity.
2. **Attention dilution caps quality.** SWE-PRBench: every tested model degraded when prompts moved from structured diff-only context to richer file/repo context. More context is not better; *selective* context is.
3. **Recall is currently low even with strong models.** SWE-PRBench: 15–31% detection of human-flagged issues. c-CRAB: ~32% pass rate for the strongest evaluated tool, 41.5% for the union of all tools. Production-grade autonomous review is not the right framing; **first-pass assistant with human oversight** is.
4. **Severity calibration is the dominant production lever.** Codex flags only P0/P1; Claude Code Review uses Important/Nit/Pre-existing; Sonar restricts AI CodeFix to selected rule sets. Tight severity discipline preserves reviewer trust and avoids workflow drag (industrial field study: comments often resolved but PR closure time rose from 5h52m to 8h20m on average).
5. **Project memory is a first-class artifact.** `CLAUDE.md`, `AGENTS.md`, `REVIEW.md`, custom instructions — every credible production system exposes a repo-level policy file. Mallard already has `CONTEXT.md` for domain language; the wedge will want a `REVIEW.md` analog for PR-review-specific policy.

A sixth point matters even though it does not change the pattern: **deterministic-hard vs LLM-soft is the right split for trust**. Linters, type checkers, SAST, dependency policies, structural rules — these are the merge-gating substrate. LLM reasoning is for semantic issues, blast-radius narration, test suggestions. Don't promote LLM-only findings to merge blockers.

## Decision

The PR review wedge is built as a **layered pipeline**:

1. **Diff parsing → changed-symbol set.** Two indexes (base SHA + head SHA), set-difference + signature-shape diff on `symbol_id` to produce added / removed / modified. Built or composed via existing primitives.
2. **Selective retrieval.** Agent composes `mallard query expand` / `neighbors` / `symbols-in-file` against the changed-symbol set per [ADR-0007](0007-defer-retrieval-module-agents-compose-primitives.md). Bounded depth + kind filters keep context minimal. Constructor noise filter already lives in `RustExtractor` ([ADR-0008](0008-heuristic-name-resolution.md)).
3. **Deterministic analyzers — hard gates.** `findings` from the ast-grep rules engine. These can be promoted to merge-blocking review comments because they are reproducible for `(SHA, rule-set hash, indexer version)`. Severity field on each rule (currently parsed, currently unused) gets surfaced to the reviewer as `P0` / `P1` / `P2` (or equivalent labels).
4. **LLM synthesis — soft signals.** The reviewer agent composes the retrieved subgraph + deterministic findings into evidence-grounded review comments. Every synthesized comment must cite the structural evidence it relies on (symbol IDs, edge paths, rule IDs), per [docs/specs/pr-review/pull-request-review.md](../specs/pr-review/pull-request-review.md).
5. **Severity-calibrated output.** Per-PR comment budget, lowest-confidence dropped first. Deterministic findings preserve their rule severity; LLM-synthesized comments self-rate but are capped by a "soft signals per PR" ceiling. Mirrors Codex P0/P1 and Claude Important/Nit discipline.
6. **Project memory.** `CONTEXT.md` carries domain language (current). When the wedge lands, add `REVIEW.md` for PR-review-specific policy: comment style, severity ceilings, suppressions (generated files, lockfiles), required evidence for certain claim categories.

Trust framing: this is a **first-pass assistant with human oversight**, not a merge gate. The deterministic-findings layer *may* gate merges; the LLM-synthesis layer does not. Public docs and PRs should describe the wedge in those terms.

## Alternatives considered

### Single-shot LLM on the diff

Pros:
- Trivial to build.
- Matches the "GPT-on-PR" baseline most teams try first.

Cons:
- Bad on every measured benchmark since 2024.
- Hallucinates references to symbols not in the diff (the "review the imaginary code" failure).
- Hits attention dilution as soon as context grows beyond the immediate hunks.

### Heavyweight retrieval module + monolithic reviewer

Pros:
- Single Rust function the wedge calls.
- Matches the original retrieval spec literally.

Cons:
- Pre-composes ranking and budget choices the agent could have made better with task knowledge.
- Closes off the agent-composition route validated by [ADR-0007](0007-defer-retrieval-module-agents-compose-primitives.md).
- Doesn't address the deterministic-vs-soft split that the research identifies as the real reliability lever.

### Layered agentic pipeline (this decision)

Pros:
- Matches the architectural pattern that every credible 2026 production system has converged on.
- Deterministic-hard layer is reproducible and gateable; LLM-soft layer is bounded and severity-calibrated.
- Each stage maps cleanly to existing or near-existing mallard surfaces: changed-symbol set (primitives), selective retrieval (CLI + Skill per [0007](0007-defer-retrieval-module-agents-compose-primitives.md)), findings (rules engine), synthesis (LLM call), memory (`CONTEXT.md` + future `REVIEW.md`).
- Project memory is text in the repo — versioned, reviewable, no special infrastructure.

Cons:
- More moving parts than a single-shot reviewer. Multiple stages = multiple places to fail.
- Severity calibration and project memory require ongoing tuning per consuming team.
- LLM-soft comments still cap at the 15–31% recall ceiling current research measures. The pipeline does not magically lift that; it bounds *false positives* and surfaces *evidence* so reviewers can triage faster.

## Consequences

Positive:
- Engine roadmap inherits a concrete shape: ship symbol-diff (when needed), ship Skill ([ADR-0007](0007-defer-retrieval-module-agents-compose-primitives.md)), surface rule severity in `findings`, draft a `REVIEW.md` template, then the wedge.
- The wedge can be honest about its quality ceiling — first-pass assistant, not merge gate — which protects trust.
- Deterministic and synthesized comments are visibly different in output, so consumers can wire merge gates only against the deterministic layer.
- Project memory is plain markdown, fits the existing CONTEXT.md / spec / decision pattern.

Negative / tradeoffs:
- Surfacing rule severity adds a tiny schema/CLI change (`findings` query needs to return severity; currently it's parsed but dropped).
- A `REVIEW.md` template is a forcing function for consuming teams to write policy — onboarding friction. Worth it; the research is clear that repo-level policy is the highest-leverage tuning surface.
- Patch validation (SRSR / OIRR / SSR style metrics) is out of scope for v0 — we don't generate patches yet. When we do, the validation stage from this pipeline becomes its own ADR.

## Related

- [0006-pr-review-as-initial-wedge.md](0006-pr-review-as-initial-wedge.md) — chose PR review as the wedge; this ADR commits to its architecture.
- [0007-defer-retrieval-module-agents-compose-primitives.md](0007-defer-retrieval-module-agents-compose-primitives.md) — the agent composes the retrieval stage.
- [0008-heuristic-name-resolution.md](0008-heuristic-name-resolution.md) — bounds the quality of the resolved-call evidence the synthesis stage can cite.
- [docs/specs/pr-review/pull-request-review.md](../specs/pr-review/pull-request-review.md) — wedge spec; will be amended to spell out the pipeline stages.
- [docs/research/agentic-code-review-tools.md](../research/agentic-code-review-tools.md) — primary research source for every claim above.
