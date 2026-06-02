# 0014 Verification layer vs comprehension layer

## Status

Accepted

## Context

ADR-0013 repositioned mallard from "AI code reviewer" to "deterministic
verifier for AI-generated code changes" and named the LLM-reviewer
competitors (CodeRabbit, Greptile, Graphite). It did **not** position
mallard against the other adjacent category that has since grown
explosively: **live code-comprehension graphs for agents**.

The prompting example is [codegraph](https://github.com/colbymchenry/codegraph)
(~37k★ by mid-2026, multi-commit/day): a TypeScript, MCP-delivered,
**live-incremental** code knowledge graph that auto-configures into eight
agents (Claude Code, Cursor, Codex, Gemini, …), covers 20+ languages, and
markets "~16% cheaper · ~58% fewer tool calls" by answering *"how does X
work now"* without the agent grepping and reading files. LSP servers and
embedding indexes occupy the same lane from different angles.

This forces a positioning question mallard had been fuzzy on: when an
agent can already get fast, broad, live structural answers from a tool
like codegraph, **what is mallard for?**

The honest answer is that these are two different axes, and mallard had
been letting messaging drift toward the crowded one:

- **Comprehension** — "what is the code, how does it work, who calls X
  *right now*." Optimised for recall and currency over a single live
  snapshot. codegraph / LSP / embeddings own this.
- **Verification** — "what *changed* between base and head, who still
  calls the symbol you removed, what shipped without a test." Optimised
  for precision, reproducibility, and audit trail over a base/head *pair*.

A live single-state index **structurally cannot** answer the verification
question well: it holds one working-tree snapshot, not two SHAs, and its
whole value proposition (always-fresh currency via a file watcher) is the
opposite design choice from "reproduce the index for SHA X six months
later." An ephemeral per-SHA index (ADR-0005) is built for exactly the
base→head delta. Trying to be excellent at both currency and
reproducibility is a genuine architectural tension, not a feature gap.

A scan of the docs found the messaging had leaked toward comprehension in
two places — `docs/system.md` led with "the bottleneck is repository
context retrieval" (generic comprehension framing), and the Agent Skill
frontmatter opened with "caller/callee graph" before the verification
job. Meanwhile `docs/plans/workflow-fit-and-contract.md` carried a stale
"No MCP wrapper" line from the May-2026 analysis, written before
MCP-native code graphs demonstrated MCP is the adoption channel.

## Decision

**Position mallard explicitly as the cross-SHA *verification layer*, and
treat comprehension tools as complementary, not competitive.**

1. **Thesis, stated everywhere:** comprehension tools (codegraph, LSP,
   embedding search) answer *"how does this code work now"*; mallard
   answers *"what changed between two SHAs, and what did it break."*
   Different axes; they compose — comprehension for authoring, mallard
   for verification.

2. **The verification finding set is the product surface that proves it.**
   The deterministic pipeline emits signals a single-state index cannot
   compute from one snapshot: `caller-drop` (removed symbol still called),
   `dead-import` (removed symbol still imported), `test-gap` (modified
   callable no test exercises), plus the modified-body family and
   structural-rule findings — all anchored to a symbol ID + file:line,
   all reproducible from the base/head index pair.

3. **MCP is a transport, not a reversal of determinism.** Supersede the
   "No MCP wrapper" line in `workflow-fit-and-contract.md`. mallard will
   ship a thin `mallard mcp` wrapper over the existing primitives so its
   verify/diff tools live in the same agents comprehension tools already
   reach. MCP carries deterministic JSON; there is still no LLM in
   mallard's surface (consistent with ADR-0011 / ADR-0013).

4. **Do not chase the comprehension axis.** No live/incremental watch
   index, no embedding fallback, no 20-language sprint to match codegraph
   on its turf. Stay tree-sitter + DuckDB + ephemeral per-SHA
   (ADR-0004 / ADR-0005). Out-*verify* comprehension tools; do not try to
   out-comprehend them.

## Alternatives considered

### Compete on comprehension (live index, broad languages, embeddings)

Pros: larger, more obviously "useful day to day" surface; matches what a
37k★ project demonstrably gets traction with.

Cons: it is codegraph's / LSP's / Sourcegraph's home turf with years of
head start and (for codegraph) enormous distribution momentum; requires
abandoning the ephemeral per-SHA model that makes verification cheap and
reproducible; recall is commoditised. Chasing it dissolves the only moat
mallard has.

### Stay silent on comprehension tools

Pros: no new ADR; ADR-0013 already names the LLM reviewers.

Cons: leaves the sharpest "why not just use codegraph" question
unanswered, and lets `system.md` / SKILL.md keep drifting toward generic
comprehension framing. The category is too large and too close to ignore.

### Position as a superset ("comprehension *and* verification")

Pros: bigger story.

Cons: dishonest about the architectural tension (currency vs
reproducibility pull in opposite directions); spreads the engine thin;
invites direct comparison on the axis mallard is weaker on.

## Consequences

Positive:
- A one-sentence answer to "why not just use codegraph": *different axis —
  comprehension vs verification.* Sales/adoption clarity.
- The caller-drop / dead-import / test-gap findings become the concrete
  proof of the position, not just prose.
- MCP wrapper unblocked as a distribution channel without compromising
  the deterministic-only identity.
- Engine roadmap stays disciplined: depth on cross-SHA verification, not
  breadth on comprehension.

Negative / tradeoffs:
- Concedes the larger, busier comprehension market to others. Deliberate.
- Requires keeping messaging honest about what mallard does *not* do
  (live in-editor answers, semantic depth) — ongoing discipline, same as
  the LSP comparison in `workflow-fit-and-contract.md`.

## Related

- [0004-symbolic-graph-retrieval-over-embeddings-first.md](0004-symbolic-graph-retrieval-over-embeddings-first.md)
- [0005-ephemeral-indexing-defer-incremental.md](0005-ephemeral-indexing-defer-incremental.md) — ephemeral per-SHA is what makes verification reproducible.
- [0006-pr-review-as-initial-wedge.md](0006-pr-review-as-initial-wedge.md)
- [0011-deterministic-only-pr-review-v1.md](0011-deterministic-only-pr-review-v1.md)
- [0013-kill-phase-d-pivot-agent-verification.md](0013-kill-phase-d-pivot-agent-verification.md) — the verification pivot this ADR extends to the comprehension-tool axis.
- `docs/plans/workflow-fit-and-contract.md` — the "No MCP wrapper" line this ADR supersedes; the LSP comparison this ADR generalises.
