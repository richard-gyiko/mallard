# Market analysis — May 2026

Three parallel research passes (competitors, buyer signals, code-graph tech) consolidated into product direction. Done after merging wedge-dogfood-1 fixes (PRs #20, #22).

## Why this document exists

After two PRs landed (Gap 2 self-impl-block resolver, Gap 3 macro-body extraction) the question shifted from "does mallard work on a real PR" to "could anyone *actually adopt* mallard given the 2026 market." This document captures the research that informed Move 1 (see [`docs/plans/move-1-python-ts-action.md`](../plans/move-1-python-ts-action.md)).

Research date: 2026-05-25.

---

## Part A — Competitive landscape

### Pricing converged tight

3–25 dev teams pay ~$20–30/seat/month. Anchors:

| vendor | $/dev/mo | notes |
| --- | --- | --- |
| CodeRabbit Pro | $24 annual / $30 monthly | 2M repos, dominant default; only bills devs who *open* PRs |
| CodeRabbit Pro+ | $48 / $60 | adds unit-test gen |
| Greptile Pro | $30 + $1/review overage | switched to per-review March 2026; "Greptile Now Charges Per Review. Nobody Else Does." — Agent Wars headline |
| Graphite Team | $40 unlimited | precision-first, "<3% FP" claim |
| Qodo Teams | $30 annual / $38 monthly | best self-host story among hosted; raised $70M March 2026 |
| GitHub Copilot Business | $19 | moves to usage-based **June 1 2026** |
| GitHub Copilot Enterprise | $39 | |
| Cursor BugBot | usage-based ~$1.00–$1.50/run | seat fees removed May 2026 |
| Sourcegraph Cody | $59 enterprise-only | killed Free + Pro **July 2025** |
| Ellipsis | $20 unlimited | explicitly moved off per-commit |
| **Anthropic Claude Code Action** | **BYOK ~$0.04–0.05/review** | ~$24/mo total for 10 devs × 20 PRs/day |

The BYOK math is the threat to vendor-managed pricing. A 10-dev team writes a $240–$300/mo check for CodeRabbit; Claude Action does the same workload for ~$24/mo with the DIY-ceiling tradeoff.

### Dominant complaint: noise

Across HN, G2, devtoolsacademy, CodeAnt:

- **Noise / over-commenting** (most-cited). HN 46777079 on Greptile: *"pretty much pure noise. I ran it for 3 PRs and then gave up."* Lychee audit: *"28% of CodeRabbit's comments were noise or incorrect assumptions."*
- **Out-of-context comments** (second-most). Same HN review: Greptile *"suggested silencing exceptions without recognizing earlier handling"* and *"flagged Python 3.14 as non-existent."*
- **False positives waste reviewer time** (quantified). CodeAnt: a 10% FPR on a 10-person team burns *"4.2 hours per week ... 218 hours wasted annually — over $65,000 in fully-loaded labor cost."* Most platforms operate at 5–15% FPR.
- **Confidence inflation.** HN comment on per-finding confidence: high (4/5) confidence on irrelevant findings causes *"productivity loss because human reviewers would assume problems existed."* Direct validation of mallard's calibrated-tier wedge.
- **Doesn't catch the real bugs.** Macroscope 2025 benchmark: leaders 42–48% bug detection. Half of bugs slip past everyone. Greptile own benchmark: CodeRabbit 44% catch, Graphite 6–18%.
- **Surprise billing under usage models.** Cursor BugBot users reported usage *"unexpectedly consumed their paid usage quota"* jumping from 20% to 99% with no explanation.

### Privacy demand is real

Hard requirement for ~20–30% of small teams (regulated verticals, financial, healthcare, defence, paranoid founders). Strong preference for another ~30%. Non-issue for the rest.

Vendors leading with privacy:
- **Kodus** — does not store source code, all processing real-time
- **Panto** — zero code retention, in-memory and immediately discarded, behind your firewall
- **Sourcery** — on-prem option
- **Cline** — *"ensuring your code never leaves your environment"*
- **Kody** — open-source, runs in your environment
- **Qodo PR-Agent** — air-gapped deployments supported
- **CodeRabbit Enterprise** (≥500 seats only) — self-hosted

### What's commoditized vs contested

**Commoditized** (everyone ships):
- Inline PR comments with suggested-change blocks
- Plain-English walkthrough + sequence/architecture summary
- Whole-repo context (diff-only is dead)
- One-click apply / fix-suggestion
- SOC2 Type II
- GitHub support

**Still contested:**
- GitLab/Bitbucket/Azure breadth (CodeRabbit + Qodo win; BugBot, Graphite, Copilot lose)
- BYOK / self-host / air-gap (Qodo PR-Agent, Anthropic Action, Sourcegraph, CodeRabbit-Enterprise only)
- Precision vs recall posture (Diamond/BugBot chase precision; Greptile chases recall; no tool dominates both)
- Pricing model (usage-based winning but burns trust when surprise bills land)

**Nobody ships well:**
- **Calibrated confidence tiers per finding.** Every tool emits comments at one undifferentiated "AI says so" level. Diamond's "<3% FP" is a marketing aggregate, not a per-comment signal. The `extracted/inferred/ambiguous/unresolved` framing has no commercial competitor.
- **Citation-required comments.** No incumbent forces every claim to cite a structural fact. Reviewers waste minutes triaging unfounded claims.
- **Truly local execution.** Sourcegraph self-hosts the index but still sends prompts to a vendor LLM. Qodo + Anthropic Action can run wherever the key points but still ship the diff out. Nobody offers *code never leaves laptop / CI runner, structural reasoning is local, LLM call is the only network hop and is auditable.*
- **Cost predictability at small-team scale.** Whipsawed between flat-fee-with-caps (Greptile $30 + overage) and usage billing (Copilot, BugBot, Anthropic Action).

---

## Part B — Buyer signals

### Buying behaviour for small dev teams (3–25)

Founder/IC-driven, trial-first, self-onboard, decide in a day. CodeRabbit's 2M repos / 13M PRs is a viral GitHub-Marketplace-install motion, not enterprise sales. Compliance enters only at the upper end of the band. >45% of devs now use AI coding tools; DX survey: average WTP $23.50/dev/mo, ceiling ~$45 for high-performers.

### Willingness to pay

- **$20–30/seat is the comfort zone** for small teams
- ~$45–60 is the ceiling
- Per-PR economics: ~$0.05/PR for AI vs. $15–25 for a human reviewer (fazm.ai)
- Free-tier-for-OSS is table stakes (CodeRabbit, Sourcery, Gemini Code Assist all offer)
- 37% of organizations preferring consumption-based for specialised AI tooling

### Integration friction

GitHub App (vendor-hosted bot) and GitHub Action (CI-step) are dominant. devtoolsacademy: *"GitHub Apps don't consume your Actions minutes, don't require YAML configuration files, run on the tool vendor's infrastructure, you install them and configure a few settings."* CodeRabbit's *"very quick setup process where users mostly had to hook it up"* is the gold standard. Anything >10 minutes of YAML wrangling loses the trial.

**Implication for mallard:** lack of one-click GitHub Action / App is the single biggest adoption gap, not the language matrix.

### "Beyond PR review" — adjacent jobs ranked by demand strength

1. **Codebase Q&A / onboarding.** Strongest. Two well-funded incumbents (Greptile, Sourcegraph Cody) validate WTP. *"Greptile's ability to provide context from the entire codebase ... helped new team members get up to speed much faster."*
2. **Blast-radius / change-impact analysis.** Direct mallard adjacency. Riftmap explicitly markets *"AI agents call into during planning to answer cross-repo questions like 'who depends on this?' and 'what's the blast radius of changing this artifact?'"*
3. **Refactoring / migration planning.** Large active market (Codemod 2.0, Grit/GritQL, Augment Code). Most LLM-only tools fake the structural piece.
4. **Architectural drift detection.** Thoughtworks Radar 2026 lists *"Architecture drift reduction with LLMs."* Drift (sauremilk/drift) launched specifically; vFunction won 2025 CODiE award.
5. **Dead-code / dependency hygiene.** Knip, Vulture, CodeAnt momentum. Lower per-customer value but ideal as free-tier hook.

Skip: incident postmortem (SRE-shaped, not dev-shaped buyer); security scanning (crowded, thin margins for small teams).

### The "structural evidence" pitch

Mixed but trending favourable:

- Academic validation: Dec 2025 paper *Citation-Grounded Code Comprehension* — *"92% citation accuracy with zero hallucinations"* using BM25 + dense embeddings + Neo4j graph expansion. 2025 arXiv *Grounded AI for Code Review* pairs *"static-analysis evidence with LLM explanations to produce citation-rich PR comments."*
- User signal: confidence scores on noise comments cause *"productivity loss"* — maps directly to mallard's tier framing.
- But: most buyers do not ask for "symbol IDs" by name. They ask for *fewer wrong comments*. Structural evidence is the means; signal-quality is the sale. Lead with outcome, prove with architecture.

---

## Part C — Code-graph tech landscape

### Direct competitors that landed in last 60 days

| project | stars | shipped | notes |
| --- | --- | --- | --- |
| **GitNexus** | 28k+ | Apr 2026 | MCP-native property graph, 12 languages, in-browser zero-server mode |
| **code-review-graph** (tirth8205) | 17.4k | v2.3.3 May 8 2026 | tree-sitter symbol + call + import graph, 24 languages, SQLite, 28 MCP tools, claims 6.8× token reduction |

Both ship the same structural-graph thesis as mallard with broader language coverage and MCP servers ready. **The infrastructure wedge has ~12-month half-life.**

### Standards landscape

- **SCIP** — Sourcegraph's Protobuf interchange format; latest 0.7.1 Apr 14 2026. De-facto standard.
- **LSIF** — deprecated; Sourcegraph drove migration to SCIP
- **GitHub Stack Graphs** — *archived Sep 9 2025.* `.tsg` grammar maintenance proved unsustainable
- **GitLab** — still LSIF; SCIP issue [gitlab#412981](https://gitlab.com/gitlab-org/gitlab/-/issues/412981) open

### Sourcegraph SMB vacuum

Cody killed Free + Pro **July 2025**, enterprise-only at $59. **The small-team segment is undefended at the privacy / on-prem + structural-grounded intersection.**

### Where mallard genuinely differentiates

1. **DuckDB persistence + composable CLI** (`jq` / `gh` / `git`). Every competitor hides the index behind MCP (GitNexus, code-review-graph, Codebase-Memory) or service API (Sourcegraph, Glean). SQL-queryable artifact you pipe in shell is a real Unix-philosophy gap.
2. **4-tier edge confidence (ADR-0010).** No surveyed tool surfaces extracted/inferred/ambiguous/unresolved as first-class metadata. code-review-graph claims *"100% recall, 0.54 F1"* — over-predicts silently. Calibration claim no one else makes.
3. **Ephemeral per-SHA index.** Sourcegraph/Glean assume long-lived server state; GitNexus/code-review-graph assume long-lived local state with hooks. Per-SHA ephemeral maps cleanly to GitHub Actions / PR review.

### Where mallard is behind

1. Single-language (Rust only) vs code-review-graph's 24 / GitNexus's 12
2. No MCP server (table-stakes for Claude Code / Cursor / Windsurf in 2026)
3. No incremental indexing (per-SHA full builds; loses at 100k LoC)
4. No LSP integration (tree-sitter heuristics less precise than rust-analyzer / scip-typescript on generics / macros)

### Worth stealing from the landscape

- **SCIP as export target** alongside DuckDB. Zero-cost interop with Sourcegraph ecosystem and future SCIP consumers.
- **Aider-style PageRank** ranking layered on the symbol graph. Cheap, deterministic, complements confidence tiers.
- **MCP server thin wrapper** around the same 10 CLI primitives. Discovery channel for the Claude Code / Cursor ecosystem.

### 18-month defensibility — honest answer

Infra wedge dies ~Q2 2027 (GitNexus + code-review-graph + Cody Memory converge). Durable moat = **synthesis quality + benchmark + brand association with calibrated review.**

Recommended defence:
- Confidence-tier benchmark vs code-review-graph + GitNexus — paper + reproducible eval suite
- SCIP export for free interop
- MCP wrapper for free discovery
- Open-core: extractor + index OSS, synthesis prompts + benchmark dataset proprietary

---

## Product direction (synthesis)

### Positioning angle

*"The AI code reviewer that knows when it's guessing."*

Confidence tiers as the trust-calibration story. Reviewers filter to `extracted-only` on a noisy PR, expand to `include ambiguous` when investigating. Solves the dominant market complaint (noise) without sacrificing recall.

### Pricing tiers (recommended)

| tier | $/dev/mo | scope |
| --- | --- | --- |
| OSS | $0 | public repos, attribution required, unlimited |
| Solo / Indie | $0 | private repos, BYOK Anthropic key, 100 reviews/mo soft cap |
| **Team** | **$9** | private repos, BYOK or vendor-managed, unlimited PR review + Q&A + blast radius |
| Self-Hosted | $19 | air-gapped binary, on-prem, structural index never leaves |

- Half CodeRabbit's price ($9 vs $24)
- Adds Q&A + blast radius vs Greptile's PR-only at $30
- Self-Hosted at $19 vs Sourcegraph $59 enterprise-only
- BYOK keeps cost ceiling user-controlled; mallard captures $7–17 management/synthesis premium, not LLM tokens

### Three moves

**Move 1 (weeks 1–8):** Python + TypeScript extractors + GitHub Action. Without JS/TS + Python, mallard is unsellable. Detailed plan: [`docs/plans/move-1-python-ts-action.md`](../plans/move-1-python-ts-action.md).

**Move 2 (weeks 9–12):** Published benchmark. Single biggest leverage. Run mallard, CodeRabbit, Greptile, Graphite, Copilot, Diamond, Devin Review, code-review-graph on same 100 merged PRs across 5 OSS repos. Public, reproducible. Submit to NeurIPS / ICSE workshop. Numbers beat marketing prose.

**Move 3 (weeks 13–20):** Expand to "code understanding platform." Same DuckDB index, four output surfaces — PR review, onboarding Q&A, blast radius, architecture drift. Repositioning: *"the local code understanding platform that knows when it's guessing."*

### Three things to NOT do

1. Don't build hosted vendor-LLM. BYOK is the moat.
2. Don't chase enterprise multi-repo. Sourcegraph occupies that. Their SMB pivot abandoned the small-team wedge — go take it.
3. Don't chase recall. Greptile's strategy. Recall = commoditized at 44–48% ceiling. Chase signal-to-noise. Trust is undefeated.

---

## Sources

Compiled from three parallel research passes (2026-05-25).

**Competitors / pricing:**
- [CodeRabbit Pricing](https://www.coderabbit.ai/pricing)
- [WeavAI CodeRabbit 2026 Review](https://weavai.app/blog/en/2026/04/30/coderabbit-2026-review-is-ai-code-review-worth-24-mo/)
- [Greptile v4 + Pricing](https://www.greptile.com/blog/greptile-v4)
- [Agent Wars: Greptile Per-Review Pricing](https://www.agent-wars.com/news/2026-05-01-greptile-per-review-pricing)
- [GitHub Copilot Plans](https://github.com/features/copilot/plans)
- [GitHub Copilot Usage-Based Billing](https://github.blog/news-insights/company-news/github-copilot-is-moving-to-usage-based-billing/)
- [Cursor BugBot May 2026 Changes](https://cursor.com/blog/may-2026-bugbot-changes)
- [Anthropic claude-code-action](https://github.com/anthropics/claude-code-action)
- [TechCrunch — Anthropic launches code review](https://techcrunch.com/2026/03/09/anthropic-launches-code-review-tool-to-check-flood-of-ai-generated-code/)
- [Graphite Agent + Pricing](https://graphite.com/blog/introducing-graphite-agent-and-pricing)
- [Qodo Pricing](https://www.qodo.ai/pricing/)
- [TechCrunch — Qodo $70M](https://techcrunch.com/2026/03/30/qodo-bets-on-code-verification-as-ai-coding-scales-raises-70m/)
- [Devin 2.2 Launch](https://cognition.ai/blog/introducing-devin-2-2)
- [Cubic](https://www.cubic.dev/)

**Buyer signals:**
- [HN: Greptile noise complaint](https://news.ycombinator.com/item?id=46777079)
- [State of AI Code Review Tools 2025 — devtoolsacademy](https://www.devtoolsacademy.com/blog/state-of-ai-code-review-tools-2025/)
- [CodeAnt: How Many False Positives Are Too Many](https://www.codeant.ai/blogs/ai-code-review-false-positives)
- [DX: AI coding assistant pricing 2025](https://getdx.com/blog/ai-coding-assistant-pricing/)
- [fazm.ai: $0.05 vs $25 per PR](https://fazm.ai/t/ai-code-review-cost-comparison)
- [Elio Struyf: AI code review journey](https://www.eliostruyf.com/ai-code-review-journey-copilot-coderabbit-macroscope/)
- [Ryz Labs Copilot Critique](https://learn.ryzlabs.com/ai-coding-assistants/why-github-copilot-is-overrated-5-things-most-developers-get-wrong)
- [Panto AI: zero code retention](https://www.getpanto.ai/blog/zero-code-retention-protecting-code-privacy-in-ai-code-reviews)
- [Kodus self-hosted](https://kodus.io/self-hosted-ai-code-review/)
- [arXiv: Citation-Grounded Code Comprehension](https://arxiv.org/html/2512.12117v1)
- [arXiv: Grounded AI for Code Review](https://arxiv.org/pdf/2510.10290)
- [Thoughtworks Radar: Architecture drift reduction with LLMs](https://www.thoughtworks.com/radar/techniques/architecture-drift-reduction-with-llms)
- [Riftmap](https://riftmap.dev/)
- [Codemod 2.0](https://codemod.com/blog/codemod2)

**Code-graph tech:**
- [Sourcegraph Cody pricing changes](https://sourcegraph.com/blog/changes-to-cody-free-pro-and-enterprise-starter-plans)
- [SCIP announcement](https://sourcegraph.com/blog/announcing-scip)
- [sourcegraph/scip GitHub](https://github.com/sourcegraph/scip)
- [GitLab SCIP issue 412981](https://gitlab.com/gitlab-org/gitlab/-/issues/412981)
- [GitNexus on MarkTechPost](https://www.marktechpost.com/2026/04/24/meet-gitnexus-an-open-source-mcp-native-knowledge-graph-engine-that-gives-claude-code-and-cursor-full-codebase-structural-awareness/)
- [tirth8205/code-review-graph](https://github.com/tirth8205/code-review-graph)
- [How Cursor Indexes Codebases Fast](https://read.engineerscodex.com/p/how-cursor-indexes-codebases-fast)
- [Aider repomap](https://aider.chat/docs/repomap.html)
- [Glean: Indexing code at scale (Meta)](https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/)
- [OpenGrep launch](https://thenewstack.io/opengrep-launches-as-free-fork-after-semgrep-license-shift/)
