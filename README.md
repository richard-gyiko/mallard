# mallard

[![ci](https://github.com/richard-gyiko/mallard/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/richard-gyiko/mallard/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/richard-gyiko/mallard?sort=semver)](https://github.com/richard-gyiko/mallard/releases)
[![license](https://img.shields.io/github/license/richard-gyiko/mallard)](LICENSE)

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

AI coding agents (Claude Code, Cursor, Copilot Workspace, Devin, Aider, Cline) now author or assist a growing share of merged PRs. Multiple 2026 studies measure quality problems — for context, not because mallard solves them all:

- 77.51% of merged agentic PRs are self-merged with no human review ([arXiv:2601.18749](https://arxiv.org/html/2601.18749))
- 22.7% of AI-introduced issues survive at HEAD ([arXiv:2603.28592](https://arxiv.org/html/2603.28592v2))
- 43% of AI-generated changes need production debugging post-QA ([Lightrun 2026](https://venturebeat.com/technology/43-of-ai-generated-code-changes-need-debugging-in-production-survey-finds))
- The dominant *structural* failure mode — *agent updated a definition but missed call sites* — is named as unsolved ([Kiro engineering](https://kiro.dev/blog/refactoring-made-right/))

Mallard targets the **structural** subset of these: callers / callees / test-seams / cross-SHA symbol diff. It does not catch logic bugs, runtime errors, textual merge conflicts, or semantic regressions. LLM-based reviewers (CodeRabbit, Greptile) cover the prose-style review and bug-pattern hunting; mallard is the deterministic structural-verification layer that complements them.

## Install — agent skill

Mallard ships as an Agent Skill discoverable via [skills.sh](https://www.skills.sh):

```bash
npx skills add richard-gyiko/mallard
```

Works with Claude Code, Codex CLI, ChatGPT, and any agent that honors the [Anthropic Agent Skills spec](https://github.com/anthropics/skills). The skill body lives at [`skills/mallard/SKILL.md`](skills/mallard/SKILL.md) and tells the agent when to call which command.

You also need the `mallard` binary on PATH. Pre-built binaries for Linux (x86_64, ARM64), macOS (x86_64, ARM64), and Windows (x86_64) are on the [Releases page](https://github.com/richard-gyiko/mallard/releases). Or build from source:

```bash
git clone https://github.com/richard-gyiko/mallard
cd mallard
cargo install --path .
```

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
      - uses: richard-gyiko/mallard/.github/actions/review@v0.1.0
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

Different products, different jobs. This table covers a narrow axis: deterministic structural verification of agent-generated changes.

|                                | mallard | CodeRabbit | Greptile | Serena    | Sourcegraph |
| ------------------------------ | :-----: | :--------: | :------: | :-------: | :---------: |
| Deterministic (no LLM)         |   ✔     |     ✘      |    ✘     |     ✔     |      ✔      |
| Per-SHA reproducible artifact  |   ✔     |     ✘      |    ✘     |     ✘     |      ✘      |
| Confidence tier per finding    |   ✔     |     ✘      |    ✘     |     ✘     |      ✘      |
| Local / zero-infra             |   ✔     |   Ent only |   Ent    |     ✔     |    Ent only |
| Anthropic Agent Skill format   |   ✔     |     ✘      |    ✘     |     ✘     |      ✘      |

What competitors do better than mallard: LLM-narrated prose reviews (CodeRabbit, Greptile), per-language semantic accuracy (LSP-backed Serena, Sourcegraph SCIP), and breadth of language coverage. Mallard is the deterministic, citation-grounded structural-diff layer — not a replacement for any of those.

## Pilot evidence

On a hand-graded 10-PR pilot ([`docs/research/wedge-dogfood-1.md`](docs/research/wedge-dogfood-1.md)), mallard's deterministic-only output scored 82% useful, 0% wrong, 100% cited.

That number anchors a quality claim, not a benchmark win. The wider benchmark — *how many agent-authored PRs ship with undetected structural defects that mallard catches* — is the [State of AI-Generated PRs](docs/research/agent-pr-quality-methodology.md) report. In progress.

## Project docs

- [`skills/mallard/SKILL.md`](skills/mallard/SKILL.md) — agent skill manifest
- [`docs/cli-json-contract.md`](docs/cli-json-contract.md) — locked v1.0 JSON schemas
- [`docs/system.md`](docs/system.md) — architecture
- [`docs/decisions/`](docs/decisions/) — ADRs (confidence tier, diff-hunk overlap, citation discipline)
- [`docs/research/agent-pr-quality-methodology.md`](docs/research/agent-pr-quality-methodology.md) — verification-gap research methodology
- [`docs/plans/workflow-fit-and-contract.md`](docs/plans/workflow-fit-and-contract.md) — current capability audit + LSP comparison

## License

MIT.
