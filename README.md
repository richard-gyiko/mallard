# mallard

**The AI code reviewer that knows when it's guessing.**

mallard ships structural-evidence PR reviews from a local DuckDB-backed
graph index over your repository. Findings carry a per-comment
**confidence tier** — `structural-rule`, `extracted`, `inferred`,
`ambiguous` — so reviewers filter noise instead of being drowned in it.

Languages: **Rust · Python · TypeScript / TSX**. More language extractors
slot into the `SymbolExtractor` seam — see [`CONTEXT.md`](CONTEXT.md).

Status: deterministic-only v1 of `mallard pr-review`. The LLM-soft
synthesis layer lands in Phase D of [the Move 1 plan](docs/plans/move-1-python-ts-action.md).

## 5-minute setup — GitHub Action

Add this to `.github/workflows/mallard-review.yml`:

```yaml
name: mallard-review
on:
  pull_request:
    types: [opened, synchronize, reopened]

jobs:
  review:
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
          rules-path: tests/fixtures/rules.yml   # optional
          max-comments: 10
```

The action runs entirely on the GitHub-hosted runner. **Code never leaves
CI.** No API key required for the deterministic-only v1.

## Local invocation

```bash
mallard index --sha "$BASE_SHA" --out base.duckdb /path/to/repo
mallard index --sha "$HEAD_SHA" --out head.duckdb /path/to/repo
mallard pr-review \
  --base-db base.duckdb \
  --head-db head.duckdb \
  --files "src/foo.rs,src/bar.py" \
  --format markdown
```

Outputs markdown with per-comment confidence-tier badges, or JSON
(`--format json`) for downstream tooling.

## Why mallard

- **Privacy-first.** Local execution, no source ships to a vendor. Fits
  regulated / paranoid teams (~20-30% of small-team buyers per the
  [2026 market analysis](docs/research/market-analysis-2026-05.md)).
- **Calibrated trust.** Per-comment confidence tier per [ADR-0010](docs/decisions/0010-edge-confidence-tier.md).
  No competitor exposes a tier per finding — they aggregate "AI says so."
- **Composable.** SQL-queryable DuckDB index. Pipe with `jq` / `gh` /
  `git` from the same shell session.
- **Structural-evidence-grounded.** Every comment cites a symbol ID or a
  rule ID. No vibes.

## How it compares

|                            | mallard | CodeRabbit | Greptile | Copilot Review |
| -------------------------- | :-----: | :--------: | :------: | :------------: |
| Per-comment confidence tier|   ✔     |     ✘      |    ✘     |       ✘        |
| Local / on-prem execution  |   ✔     |   Ent only |   Ent    |       ✘        |
| BYOK (own API key)         | Phase D |     ✘      |    ✘     |       ✘        |
| Free for OSS               |   ✔     |    Limited |   ✘      |       ✔        |

See [the market analysis](docs/research/market-analysis-2026-05.md) for
pricing detail and the structural-graph landscape.

## Project docs

- [`docs/system.md`](docs/system.md) — architecture
- [`docs/plans/move-1-python-ts-action.md`](docs/plans/move-1-python-ts-action.md) — current roadmap
- [`docs/decisions/`](docs/decisions/) — ADRs
- [`docs/research/`](docs/research/) — wedge dogfood + market analysis
- [`docs/specs/pr-review/pull-request-review.md`](docs/specs/pr-review/pull-request-review.md) — pr-review wedge contract

## License

Not yet declared — see open issue to choose between MIT / Apache-2.0 / dual.
