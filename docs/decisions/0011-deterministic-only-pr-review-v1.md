# 0011 Deterministic-only PR review v1 (no LLM in CI)

## Status

Accepted

## Context

mallard's `mallard pr-review` subcommand and the `mallard-review` GitHub
Action ship the first end-to-end product surface (Move 1 Phase C). The
question for v1 was whether to:

1. Ship deterministic-only review (stages 3–5 from the pr-review skill
   spec: signature diff, modified-body edge diff, structural-rule
   findings) — no LLM call.
2. Ship LLM-soft synthesis bundled in (calls Anthropic API from the
   action) for richer review comments.

Two pressures pull opposite directions:

- The 2026 market analysis (internal)
  finds the dominant small-team complaint about AI reviewers is
  **noise**, not lack of insight. CodeRabbit's "28% comments noise"
  (Lychee audit) and Greptile's "pure noise, gave up after 3 PRs" (HN
  46777079) are the prevailing user signal. LLM-soft synthesis without
  calibration tooling makes mallard part of the problem.
- The same analysis finds **~20–30% of small teams hard-require
  on-prem / local-runnable** code review. Sourcegraph's July 2025 SMB
  pivot to enterprise-only at $59/seat vacated that segment. Anthropic's
  Claude Code Action ships BYOK at ~$2.40/dev/mo but is LLM-only — no
  structural reasoning.

## Decision

**Ship deterministic-only v1.** The `mallard pr-review` subcommand and
the `mallard-review` GitHub Action run stages 3–5 entirely on the runner
with **zero network calls**. LLM-soft enrichment lands in Phase D as an
optional layer keyed off `--anthropic-api-key`, opt-in by user, off by
default.

Per-comment confidence tier from ADR-0010 surfaces as a markdown badge
prefix: `[structural-rule]` · `[extracted]` · `[inferred]` · `[ambiguous]`.
Reviewers filter on tier in the GitHub UI by scanning prefixes.

Comment-budget tier priority (highest signal kept, lowest dropped under
`max-comments`):
1. `structural-rule` — deterministic rule match, irrefutable
2. `extracted` — intra-file resolution at parse time
3. `inferred` — post-build cross-file resolution
4. `ambiguous` — multiple candidates, reviewer disambiguates
5. `unresolved` — typically stdlib / external, deprioritised

## Alternatives considered

### Ship LLM-soft synthesis in v1

**Pros**:
- Richer comments out of the box; competitive with CodeRabbit's
  walkthrough on first impression.
- Validates the synthesis prompts against confidence tiers (the
  defensible 18-month moat per ADR-0010 + market analysis).

**Cons**:
- API key handling, rate limiting, cost budgeting all become v1
  problems. Each adds setup friction past the "5-minute install" bar.
- Privacy-sensitive segment can't adopt — exactly the segment mallard
  is best positioned to serve.
- The deterministic-only tier alone is novel: no competitor exposes
  per-comment confidence tier today. Shipping it first proves the
  trust-calibration story without LLM noise muddying the signal.

### Wrap Anthropic's claude-code-action

**Pros**:
- Reuses existing LLM orchestration. No HTTP client to write.

**Cons**:
- Heavyweight CI dep (Bun runtime, claude-code installation).
- mallard's structural index becomes a side-channel input rather than
  the synthesis driver.
- Couples mallard to Anthropic's tooling lifecycle.

### Defer all PR-review surfacing to a Phase D big-bang

**Pros**:
- Single PR to land with both deterministic + LLM layers.

**Cons**:
- Move 1's success criterion is "external user installs the action and
  sees an inline review in <10 min." Big-bang blocks that signal.

## Consequences

Positive:
- **Privacy wedge intact.** Source code never leaves the CI runner in
  v1. The action is auditable end to end (composite YAML + open-source
  Rust CLI).
- **Cost story trivial.** $0 per PR — no LLM tokens. Adoption decision
  by small teams reduces to "do we want structural findings + tiered
  trust signals on our PRs."
- **Per-comment confidence tier is the headline.** Empirically validated
  against the dominant market complaint (noise) — the badge prefix
  makes the calibration story visible at the first PR review.
- **Phase D LLM enrichment becomes additive**, not a replacement. Users
  who want richer comments set `anthropic-api-key`; users who don't,
  stay on the free + private tier.

Negative / tradeoffs:
- Comments are sparser than competitors' walkthrough format. Stories
  like "you should refactor this into smaller functions" can't be
  expressed without the LLM layer. v1 surfaces structural facts only.
- Pure modified-body deltas can be cryptic without semantic context.
  Mitigated by citing symbol qualified names + the changed-callee list
  in the comment body.
- The "wow factor" of the first PR review is muted. Compensated by
  the speed (no API round-trip) and the unique calibration UI.

## Related

- [ADR-0009](0009-pr-review-architecture-pattern.md) — the layered
  pipeline definition. v1 ships stages 3, 4, 5; 6 (LLM synthesis) lands
  Phase D.
- [ADR-0010](0010-edge-confidence-tier.md) — the four-tier confidence
  model that becomes the badge prefix.
- 2026 market analysis (internal) — the noise vs. recall framing that
  drives this decision.
- Move 1 plan (internal) — Phase C / Phase D split, since superseded
  by [ADR-0013](0013-kill-phase-d-pivot-agent-verification.md).
