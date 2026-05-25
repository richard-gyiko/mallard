# ADR-0013 — Kill Phase D LLM-soft synthesis; pivot to agent-verification distribution

**Status:** accepted, 2026-05-25
**Supersedes:** the Phase D LLM-soft synthesis path described in [ADR-0011](0011-deterministic-only-pr-review-v1.md) and the prior Move 1 plan.

## Context

ADR-0011 (2025-Q4) shipped mallard's v1 as deterministic-only PR review, with a planned **Phase D** that would add bundled LLM-soft synthesis once the deterministic floor was validated. The plan was: v1 deterministic-only → Phase D LLM-soft → CodeRabbit-competitive product.

Validation arrived in early 2026:

- 10-PR hand-graded pilot — 82% useful, 0% wrong, 100% cited on deterministic-only output.
- Competitor landscape consolidated: CodeRabbit ($60M Series B, $40M ARR / 8k paying customers Sep 2025), Greptile ($25M Benchmark Sep 2025), Graphite ($52M Mar 2025). PR-review category is funded, crowded, and decided.
- Independent benchmarks ([Scalekit](https://www.scalekit.com/blog/mcp-vs-cli-use), [Apideck](https://www.apideck.com/blog/mcp-server-eating-context-window-cli-alternative)) show local-CLI tools beat MCP wrappers on tokens (1.3x–80x) and reliability (100% vs 72%) for stable-interface workloads.
- Anthropic Skills (Oct 2025) + OpenAI Codex CLI adoption (Dec 2025) created a low-token, no-MCP distribution channel for local-CLI tools.
- Quantitative agent-PR defect data emerged: 27.67% of AI-agent PRs produce merge conflicts ([AgenticFlict arXiv:2604.03551](https://arxiv.org/html/2604.03551)); 77.51% of merged agentic PRs self-merge with no human review ([arXiv:2601.18749](https://arxiv.org/html/2601.18749)); 22.7% of AI-introduced issues survive at HEAD ([arXiv:2603.28592](https://arxiv.org/html/2603.28592v2)). The dominant failure mode — *agent updated a definition but missed call sites* — is named as unsolved ([Kiro engineering](https://kiro.dev/blog/refactoring-made-right/)).

Two implications:

1. **Phase D is the wrong product direction.** Adding LLM-soft synthesis makes mallard look like a budget CodeRabbit. The category is decided; mallard cannot win it. LLM synthesis also negates mallard's only unique asset: zero hallucination by construction.
2. **Agent verification is an unclaimed category.** No funded competitor is positioned at "deterministic verification of agent-generated code changes." Mallard's existing assets — per-SHA reproducibility, citation discipline, confidence tiers, diff-aware structural deltas, no-LLM guarantee — map 1:1 to this need.

## Decision

**Kill Phase D.** No bundled LLM-soft synthesis. Ever. Mallard's pitch becomes "we don't hallucinate" — adding an LLM layer would surrender that.

**Reposition** from "AI code reviewer" to "deterministic verifier for AI-generated code changes." Headline tagline: *"Verify what your AI agent actually changed."*

**Distribute via Agent Skill** (Anthropic spec, OpenAI-adopted). Single `SKILL.md` at `skills/mallard/SKILL.md` reaches Claude Code, Codex CLI, ChatGPT. Other agents (Cline, Aider, OpenCode) shell-exec the CLI directly — no per-agent recipes. No MCP wrapper.

**Lock CLI JSON contract v1.0** for the four agent-facing primitives: `query find`, `query blast-radius`, `query test-seams`, `symbol-diff`. Schemas documented in [`docs/cli-json-contract.md`](../cli-json-contract.md). Power-user primitives (per ADR-0007) keep their existing shape for back-compat.

## Consequences

- ADR-0011's "Phase D as ramp" framing is obsolete. ADR-0011's deterministic-only v1 decision stands and becomes permanent product shape, not a temporary phase.
- ADR-0009's "deterministic-hard + LLM-soft" architecture pattern remains valid as an analytical framework, but mallard explicitly chooses to ship only the deterministic-hard tier.
- Pricing plan simplifies to two tiers: OSS (this repo, MIT, free forever) and a future Team Cloud (hosted index cache + dashboard, deferred until OSS adoption validates demand).
- Phase D budget (estimated ~1 month engineering) redirects to: agent-PR research dataset (State-of-AI-PRs report) + skills.sh distribution + agent-detection ruleset in the GitHub Action.
- Move 2 (codebase-health trend tracking) becomes the natural next product layer instead of LLM-soft synthesis. Go/no-go gated on differential analysis from State-of-AI-PRs Part B.

## Alternatives considered

### Ship Phase D anyway, narrow to a privacy/on-prem segment

Would put mallard in the ~20–30% of small-team buyers segment ADR-0011 originally targeted. Buyers exist but the segment is saturated by Tabnine, Qodo, CodeRabbit-Enterprise, PR-Agent. Selling into it requires SOC2, mid-five-figure ACVs, procurement cycles measured in quarters — solo-builder-hostile GTM. Rejected.

### Ship Phase D, compete head-on with CodeRabbit

Funded incumbents have a 2-year head start, well past 8-figure aggregate funding across the category, 30+ language coverage, and polished UX. Mallard would be a budget-CodeRabbit at best. Rejected.

### Build MCP server as primary distribution

Independent benchmarks show MCP burns 1.3x–80x more tokens than CLI for stable-interface workloads. Perplexity pulled MCP internally March 2026 citing "72% context waste." Anthropic shipped progressive disclosure for MCP in Jan 2026 — implicit admission the schema-tax problem was real. For a single Rust CLI with no auth requirements, MCP is pure overhead. Rejected. Skill-format distribution chosen instead. (May add a thin MCP stdio wrapper later if a hosted-agent user requests it — small, late, optional.)

### Stay PR-review-product, just polish

Caps mallard's ceiling at the unfunded fringe of a category where three competitors are already past 8-figure-aggregate funding and competing on UX + breadth. Mallard has no edge in either dimension. Rejected.

## Validation criteria

This ADR's bet pays off if:

- **8 weeks post-launch:** ≥ 1k `npx skills add richard-gyiko/mallard` installs.
- **State-of-AI-PRs report Part A** shows ≥ 10% per-agent caller-drop rate that mallard catches.
- **12 months:** ≥ 1 named agent vendor responds publicly (acknowledgment or defense); ≥ 5k HN points on the launch / report; ≥ 1 enterprise inquiry about hosted Team Cloud.

If none → repositioning thesis is wrong and a follow-up ADR documents the next pivot.

## References

- [ADR-0006](0006-pr-review-as-initial-wedge.md) — PR review as initial wedge (still valid as the wedge that validated the underlying engine).
- [ADR-0007](0007-defer-retrieval-module-agents-compose-primitives.md) — agents compose primitives via CLI (now the load-bearing distribution model).
- [ADR-0009](0009-pr-review-architecture-pattern.md) — layered pipeline analytical framework.
- [ADR-0010](0010-edge-confidence-tier.md) — confidence tier model.
- [ADR-0011](0011-deterministic-only-pr-review-v1.md) — deterministic-only v1, Phase D framing now superseded.
- [`docs/cli-json-contract.md`](../cli-json-contract.md) — locked agent-facing JSON contract.
- [`skills/mallard/SKILL.md`](../../skills/mallard/SKILL.md) — agent skill manifest.
- [`docs/research/agent-pr-quality-methodology.md`](../research/agent-pr-quality-methodology.md) — State-of-AI-PRs report design.
