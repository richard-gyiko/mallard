# mallard

**Verify what your AI agent actually changed.**

Mallard is a deterministic, citation-grounded code-index for the era where AI coding agents author a large share of merged PRs. It catches the structural defects agents introduce — renamed symbols with abandoned callers, removed functions still imported, modified behaviors with no test update — using a per-SHA DuckDB graph index. Local. Single binary. Zero LLM. Every result anchored to a symbol ID + `file:line`.

```text
Agent commits PR → mallard runs → structural delta posted
─────────────────────────────────────────────────────────
🦆 Removed `auth_check` — still called at api/handlers.py:42
🦆 Renamed `parse_token` → `parseToken` — 3 of 4 callers updated
🦆 Modified `validate_user` — no test changes detected
```

Languages: **Rust · Python · TypeScript · JavaScript**.

## Why this exists

AI coding agents (Claude Code, Cursor, Copilot Workspace, Devin, Aider, Cline) now author or assist a growing share of merged PRs. The output has measurable structural-quality issues:

- **27.67% of AI-agent PRs produce merge conflicts** — Copilot 15%, Cursor 20%, Devin 23%, Claude Code 26%, Codex 32% ([AgenticFlict, arXiv:2604.03551](https://arxiv.org/html/2604.03551))
- **77.51% of merged agentic PRs are self-merged** (no human review) ([arXiv:2601.18749](https://arxiv.org/html/2601.18749))
- **22.7% of AI-introduced issues survive at HEAD** ([arXiv:2603.28592](https://arxiv.org/html/2603.28592v2))
- **43% need production debugging post-QA** ([Lightrun 2026](https://venturebeat.com/technology/43-of-ai-generated-code-changes-need-debugging-in-production-survey-finds))
- The dominant failure mode — *agent updated a definition but missed call sites* — is named as unsolved ([Kiro engineering](https://kiro.dev/blog/refactoring-made-right/))

LLM-based reviewers (CodeRabbit, Greptile) cannot deterministically verify agent output — they hallucinate by construction. Manual review doesn't scale at agent throughput.

The gap: **deterministic verification of agent-generated code changes.**

## Install — agent skill

Mallard ships as an Agent Skill discoverable via [skills.sh](https://www.skills.sh):

```bash
npx skills add richard-gyiko/mallard
```

Works with Claude Code, Codex CLI, ChatGPT, and any agent that honors the [Anthropic Agent Skills spec](https://github.com/anthropics/skills). The skill body lives at [`skills/mallard/SKILL.md`](skills/mallard/SKILL.md) and tells the agent when to call which command.

You also need the `mallard` binary on PATH. Build from source:

```bash
git clone https://github.com/richard-gyiko/mallard
cd mallard
cargo install --path .
```

Pre-built binaries land in GitHub Releases once `v0.1.0` is cut.

## Quickstart — direct CLI

```bash
# Build a per-SHA index
mallard index --sha "$(git rev-parse HEAD)" --out .mallard/head.duckdb .

# Pre-refactor: what breaks if I touch this symbol?
mallard query blast-radius --index .mallard/head.duckdb --qname auth_check

# Find a symbol by qualified name
mallard query find --index .mallard/head.duckdb --qname auth_check

# Which tests exercise this symbol?
mallard query test-seams --index .mallard/head.duckdb --qname auth_check

# What symbols changed between two SHAs?
mallard symbol-diff --base-db base.duckdb --head-db head.duckdb
```

All commands emit JSON on stdout with `schema_version: "1.0"`. Full schema reference: [`docs/cli-json-contract.md`](docs/cli-json-contract.md).

## GitHub Action — verify agent PRs in CI

```yaml
name: mallard-verify
on:
  pull_request:
    types: [opened, synchronize, reopened]

jobs:
  verify:
    runs-on: ubuntu-latest
    permissions:
      pull-requests: write
      contents: read
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: richard-gyiko/mallard/.github/actions/review@main
        with:
          max-comments: 10
```

Runs on GitHub-hosted runner. **Code never leaves CI.** No API key. No LLM.

## What mallard is NOT

- **Not a PR reviewer for humans.** CodeRabbit and Greptile do that well. Mallard catches what they can't: deterministic structural verification.
- **Not a bug-finder.** Mallard sees structure, not semantics. Logic bugs that compile and structurally type-check slip through.
- **Not a SAST tool.** Use Semgrep / Snyk / CodeQL for security patterns.
- **Not LLM-narrated.** Facts with citation anchors or silence — no "I think this might be a problem."
- **Not an LSP.** LSP answers "what's at this cursor right now"; mallard answers "what changed between these two SHAs, who called the symbol you just removed, and can the reviewer audit this finding 6 months from now." Use LSP for live editing. Use mallard for cross-SHA diff verification, blast-radius scoping, and CI-time structural audits.

## How mallard compares

|                                | mallard | CodeRabbit | Greptile | Serena    | Sourcegraph |
| ------------------------------ | :-----: | :--------: | :------: | :-------: | :---------: |
| Deterministic (no LLM)         |   ✔     |     ✘      |    ✘     |     ✔     |      ✔      |
| 100% citation discipline       |   ✔     |     ✘      |    ✘     |     ✘     |     ✔ (ent) |
| Per-SHA reproducible artifact  |   ✔     |     ✘      |    ✘     |     ✘     |      ✘      |
| Confidence tier per finding    |   ✔     |     ✘      |    ✘     |     ✘     |      ✘      |
| Diff-aware structural deltas   |   ✔     |     ✔      |    ✔     |     ✘     |      ✘      |
| Local / zero-infra             |   ✔     |   Ent only |   Ent    |     ✔     |    Ent only |
| Agent Skill (low-token)        |   ✔     |     ✘      |    ✘     |     ✘     |      ✘      |
| Cross-language unified graph   |   ✔     |     ✔      |    ✔     |     ✘     |      ✘      |

## Pilot evidence

On a hand-graded 10-PR pilot ([`docs/research/wedge-dogfood-1.md`](docs/research/wedge-dogfood-1.md)), mallard's deterministic-only output scored 82% useful, 0% wrong, 100% cited.

That number anchors a quality claim, not a benchmark win. The wider benchmark — *how many agent-authored PRs ship with undetected structural defects that mallard catches* — is the [State of AI-Generated PRs](docs/research/agent-pr-quality-methodology.md) report. In progress.

## Pricing

- **OSS (this repo):** CLI + Agent Skill + GitHub Action. Free forever. MIT.
- **Team Cloud (planned):** hosted index cache, agent-PR regression dashboard, merge-gate policies. $15/dev/mo. Ships when OSS validates demand.

## Project docs

- [`skills/mallard/SKILL.md`](skills/mallard/SKILL.md) — agent skill manifest
- [`docs/cli-json-contract.md`](docs/cli-json-contract.md) — locked v1.0 JSON schemas
- [`docs/system.md`](docs/system.md) — architecture
- [`docs/decisions/`](docs/decisions/) — ADRs (confidence tier, diff-hunk overlap, citation discipline)
- [`docs/research/agent-pr-quality-methodology.md`](docs/research/agent-pr-quality-methodology.md) — verification-gap research methodology
- [`docs/plans/workflow-fit-and-contract.md`](docs/plans/workflow-fit-and-contract.md) — current capability audit + LSP comparison
- [`next.md`](next.md) — strategy + 10-week plan

## License

MIT.
