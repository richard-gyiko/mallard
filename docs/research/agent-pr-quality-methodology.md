# State of AI-Generated PRs — methodology

**Goal:** Produce a publishable report with concrete numbers on the structural-quality gap in AI-agent-generated PRs. Output anchors mallard's repositioning from "PR reviewer" to "agent-PR verifier." Target publish: 6 weeks from kickoff.

## Prior art — what's already published (mid-2026)

This report extends, does not duplicate, the existing literature:

| source | finding | what it does NOT measure |
|---|---|---|
| [AgenticFlict (arXiv 2604.03551)](https://arxiv.org/html/2604.03551) | 27.67% of AI-agent PRs produce textual merge conflicts (Copilot 15%, Cursor 20%, Devin 23%, Claude Code 26%, Codex 32%) | structural breakage with no textual conflict |
| [arXiv 2601.18749](https://arxiv.org/html/2601.18749) | 77.51% of merged agentic PRs are self-merged (no independent review) | what would a deterministic verifier have caught |
| [GitClear 2025](https://www.gitclear.com/ai_assistant_code_quality_2025_research) | copy-paste rate 8.3% → 12.3% since 2021; refactoring 25% → <10% | per-symbol structural impact |
| [CodeScene / Borg-Tornhill Jan 2026](https://codescene.com/hubfs/whitepapers/AI-Ready-Code-How-Code-Health-Determines-AI-Performance.pdf) | AI assistants raise defect risk ≥30% in unhealthy code | citation-grounded structural defect inventory |
| [Lightrun 2026](https://venturebeat.com/technology/43-of-ai-generated-code-changes-need-debugging-in-production-survey-finds) | 43% of AI-generated changes need post-QA production debugging | which defect classes |
| [arXiv 2603.28592](https://arxiv.org/html/2603.28592v2) | 22.7% of AI-introduced issues survive at HEAD | structural-class breakdown |
| [Kiro engineering](https://kiro.dev/blog/refactoring-made-right/) | explicitly names missed-caller failure mode as unsolved | quantified frequency |

**Our novel contribution:** quantified, cite-grounded structural defect inventory per agent — specifically the caller-drop / test-seam abandonment / dead-import classes that no existing dataset isolates.

## Headline question

> *Of the AI-agent-generated PRs that get merged into popular OSS repos, what fraction contain undetected structural defects that mallard would have caught?*

Concrete sub-questions:

1. **Caller-drop rate** — PR renames/removes a symbol; some callers remain unupdated.
2. **Test-seam abandonment rate** — PR modifies tested behavior without touching the test.
3. **Dead-import rate** — PR imports symbols that disappear in same PR.
4. **Confidence-tier-1 finding rate** — mallard structural-rule violations introduced by the agent.
5. **Reviewer-miss rate** — of the above, how many shipped past human review.

## Dataset construction

### Source pools

| pool | size | detection signal (validated 2026-05) |
|---|---|---|
| Copilot Workspace / Coding Agent | ~500 | author = "GitHub Copilot" bot, **signed commits since 2026-04-03**, `Co-authored-by: Copilot` trailer ([source](https://github.com/orgs/community/discussions/164099)) |
| Cursor background agent | ~200 | author email matches `cursoragent@cursor.com` OR `cursoragent@users.noreply.github.com` ([source](https://github.com/anysphere/cursor-wiki/blob/main/Commit-Signing.md)) |
| Devin | ~100 | dedicated per-org bot user, author/email match ([source](https://docs.devin.ai/integrations/gh)) |
| Claude Code | ~300 | trailer `Co-Authored-By: Claude <noreply@anthropic.com>` (sometimes model-versioned) ([source](https://fabiorehm.com/blog/2026/03/02/our-coding-agent-commits-deserve-better-than-co-authored-by/)) |
| Aider | ~200 | author/committer **name** suffix `(aider)` OR opt-in `Co-Authored-By: aider` ([source](https://aider.chat/docs/git.html)) |
| Cline | ~100 | bot/MCP signature in commit; identify via Cline marketplace adopters |
| **Total target** | **~1,400 PRs** | |

Detection ruleset = union of (author email regex `cursoragent|copilot|devin|github-actions[bot]`) + (trailer match `Co-Authored-By: Claude|Copilot|Aider`) + (author name suffix `(aider)`) + configurable Devin org-user list. Covers >95% of agent PRs today.

### Repo selection criteria

- Public GitHub, MIT/Apache-2.0/BSD licensed (research-fair-use safe regardless, but cleaner).
- Languages in mallard scope: Rust, Python, TypeScript, JavaScript.
- ≥ 1k stars (filters toy repos; ensures real review process).
- Merged PRs from past 6 months (post 2025-12-01).
- Exclude: vendor-driven repos (e.g., Anthropic's own Claude Code repo) — conflict-of-interest.

### Sampling strategy

Stratified sample by:
- Agent (6 buckets above)
- Language (4 buckets)
- Repo size tier (small / medium / large)

≥ 30 PRs per non-empty cell → statistical floor for percentages.

## Measurement protocol

For each PR:

1. **Index base SHA + head SHA** with `mallard index`.
2. **Run `mallard pr-review`** with full ruleset, `--max-comments=unlimited`.
3. **Run `mallard diff-hunks`** to extract structural deltas.
4. **Categorize findings** by sub-question (1–4 above).
5. **Confirm reviewer-miss** — was finding raised in actual PR review comments? (Use `gh pr view <n> --json comments`.)
6. **Manual grade sample** — random 10% of flagged findings, two graders, label true/false positive. Calibrates mallard precision on this dataset (different distribution from pilot v7).

## Output metrics — Part A (per-PR, Move 1 validation)

| metric | definition | target precision |
|---|---|---|
| caller-drop rate | % of PRs with ≥ 1 unupdated caller after symbol rename/removal | ±3pp at 95% CI |
| test-seam abandonment rate | % of PRs modifying tested fn without touching its test | ±3pp |
| dead-import rate | % of PRs introducing import of symbol removed in same PR | ±3pp |
| tier-1 violation rate | % of PRs with ≥ 1 structural-rule violation | ±3pp |
| reviewer-miss rate | % of above NOT caught in human review comments | ±5pp |
| per-agent breakdown | each metric, sliced by agent | ±5pp per slice |

## Output metrics — Part B (cumulative trend, Move 2 validation)

Single PRs ≠ whole story. Even when each agent PR passes per-PR checks, codebases collaboratively maintained with agents may rot at the trend level: duplication accumulates, dead code piles up, coupling sprawls, hot symbols emerge. Part B measures this — and serves as the empirical basis for whether [Move 2](../../next.md#move-2--codebase-health-for-agent-collaborated-repos-q3-2026) is justified.

### Sub-corpus for trend analysis

Filter dataset to repos with **high agent-PR density** — ≥ 30% of merged PRs in last 6 months are agent-authored per detection ruleset. Estimate yield: ~50-80 repos from main 1,400-PR pool. Stratify by language.

For each such repo, build a **SHA timeline**:
- T0 = SHA 12 months ago (pre-agent-collaboration baseline OR earliest agent PR, whichever is later)
- T1, T2, ..., Tn = every merge SHA across 6-12 months, at most ~200 SHAs per repo (subsample evenly if more)

Run `mallard index` against every SHA in the timeline. Store `.duckdb` per SHA.

### Trend metrics

| metric | computed how | what it tests |
|---|---|---|
| **Duplication index trend** | symbol AST-fingerprint clustering — count near-duplicate symbols per SHA, plot over time | does duplication accumulate? |
| **Dead-code accumulation** | symbols with inbound-edges=0 per SHA, plot count over time | do unused public symbols pile up? |
| **Caller-fanout outliers** | track top-decile inbound-edge symbols across SHAs — do "god functions" emerge / grow? | does coupling concentrate? |
| **Structural-rule violation density** | (rule findings) / kloc per SHA | does rule-violation density trend up? |
| **Cross-module coupling drift** | edges crossing module boundaries / total edges, per SHA | does modularity erode? |
| **Test-seam coverage drift** | (symbols with test seams) / (public symbols), per SHA | does test coverage of API surface drift? |
| **Graph-complexity drift** | mean call-graph depth + fanout per symbol, per SHA | does structural complexity grow? |

### Differential analysis — agent-PR cohort vs human-PR cohort

For each metric, compute delta at SHA boundary:
- `Δ_agent_PR` = metric change across SHAs whose PR was agent-authored
- `Δ_human_PR` = metric change across SHAs whose PR was human-only

Test: is `Δ_agent_PR > Δ_human_PR` significantly per metric, per repo, per language?

If YES across most metrics → cumulative rot is empirically real → Move 2 justified.
If NO / mixed → drop Move 2 or reshape it.
If only some metrics show signal → ship Move 2 narrowly (only the validated metrics).

### Statistical guardrails

- Per-repo confounders (refactor sprints, version bumps, test additions): use repo as random effect in mixed model.
- Time confounders (general codebase aging): include time-since-T0 as covariate.
- Selection bias (repos that adopted agents heavily may be different): publish repo-list openly; reproducible.
- Publication bias toward sensational finding: pre-register that null result will be published as null.

### Report structure under combined Part A + Part B

| section | content | conclusion supports |
|---|---|---|
| §1 Headline | per-PR defect rates, per-agent breakdown | Move 1 wedge |
| §2 Reviewer-miss | how many slipped past humans | Move 1 wedge |
| §3 Per-class breakdown | caller-drops, test-seam abandons, dead imports | Move 1 wedge |
| §4 Trend metrics | duplication / dead-code / coupling drift over 6-12 mo | Move 2 hypothesis |
| §5 Differential analysis | agent-PR Δ vs human-PR Δ | Move 2 go/no-go |
| §6 Methodology + dataset release | reproducibility + open dataset on Zenodo | credibility |

### Compute & time impact

Part B adds significant compute:
- ~50-80 repos × ~200 SHAs × ~10s index = ~28-44 hours indexing
- Storage: ~10-20 GB DuckDB total
- Analysis: ~1 week extra (trend computation + diff testing)

Total methodology timeline 6 weeks → **8 weeks** with Part B included.

### What this protects against

Without Part B: ship Move 1, succeed → build Move 2 → discover hypothesis was wrong → wasted 4-6 weeks.

With Part B: same dataset effort validates BOTH moves with one publication. Move 2 ships only if data supports it. Free option.

## Honesty constraints

- **Pre-register protocol** before running. Publish methodology and metric definitions before opening database. Prevents p-hacking post-hoc.
- **Publish negative results.** If rate < 5%, publish the null finding. Credibility > narrative.
- **Open-source dataset** (PR URLs + mallard outputs). Reproducibility = trust.
- **Disclose mallard precision on dataset.** If pilot-v7 82% drops to 60% on broader sample, say so.
- **Manual-grade by two people minimum.** Solo grading = bias suspect. Recruit one external reviewer (offer co-author credit).
- **Name agent vendors per-metric.** Precedent: [AgenticFlict (arXiv 2604.03551)](https://arxiv.org/html/2604.03551) published per-vendor numbers (Copilot 15%, Cursor 20%, Devin 23%, Claude Code 26%, Codex 32%). Anonymizing here = weaker signal + slower category adoption. Risk: vendor pushback. Mitigation: pre-publication 7-day reach-out to each named vendor for fact-checks (not approval) — surfaces methodology disputes before launch, signals good faith.

## Risks

| risk | mitigation |
|---|---|
| Agent-PR detection misclassifies | Cross-validate against author + trailer + manual spot-check |
| Selected repos non-representative | Stratify; document selection bias |
| mallard false-positive rate inflates findings | Manual-grade subsample; report mallard precision alongside |
| Vendor pushback on named numbers | 7-day pre-publication reach-out for fact-checks (not approval); document any disputes inline; AgenticFlict precedent already shipped per-vendor numbers without incident |
| Defamation / antitrust complaints | Stick to objective metrics with reproducible methodology; no qualitative judgments; dataset open for re-analysis; methodology pre-registered |
| Story gets "AI bad" twisted | Frame as "verification gap" + "mallard catches this" — not "agents are bad". Publish mallard's own miss rate too |
| One vendor refuses fact-check engagement | Publish anyway with "vendor did not respond by [date]" note; AgenticFlict precedent |

## Publication plan

- **Pre-print** on arXiv (cs.SE).
- **Blog version** on mallard site.
- **Companion HN submission** — title: "We analyzed 1,400 AI-agent PRs. N% have undetected structural defects."
- **Dataset on Zenodo** for DOI.
- **Per-vendor breakdown PUBLISHED** — agents named (GitHub Copilot, Cursor, Devin, Claude Code, Codex, Aider, Cline). Decision locked.
- **Per-vendor pre-publication reach-out** — 7 days before launch, email each named vendor with their numbers + methodology. Request fact-checks (not approval). Document any disputes inline in final report. Vendors who don't respond by deadline → publish anyway with "no response" note. AgenticFlict precedent.

## Budget

**Part A (per-PR):**
- Compute: 1,400 PRs × 2 SHAs × ~10s = ~8 hours single-thread. Trivial.
- Storage: ~5 GB DuckDB. Trivial.

**Part B (trend):**
- Compute: 50-80 repos × ~200 SHAs × ~10s = ~28-44 hours. Run overnight batches.
- Storage: ~10-20 GB DuckDB. Trivial.

**Total time:** 2 weeks dataset build (A), 2 weeks dataset build (B), 1 week per-PR analysis, 1 week trend analysis, 2 weeks writeup + iteration = **8 weeks** solo.

## Success criteria

Report ships AND (one of):
- ≥ 1 named agent vendor responds publicly (acknowledgment or defense)
- ≥ 5k HN points / front-page placement
- ≥ 3 inbound integration requests
- ≥ 1 enterprise inquiry about hosted mallard

If none → Move 1 positioning thesis is wrong. Iterate.

**Move 2 go/no-go criterion (additional):**
- Differential analysis shows `Δ_agent_PR > Δ_human_PR` significantly (p < 0.05, Bonferroni-corrected for 7 metrics) on ≥ 3 of 7 trend metrics → ship Move 2 in Q3
- ≥ 1 but < 3 metrics significant → ship narrow Move 2 limited to those metrics
- 0 significant → drop Move 2; codebase rot hypothesis falsified; focus on Move 1 polish and Team Cloud build
