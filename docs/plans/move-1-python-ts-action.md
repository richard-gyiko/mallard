# Move 1 — Python + TypeScript extractors + GitHub Action

Target: 8 weeks. Goal: mallard becomes installable on a JS/TS or Python repo via a one-YAML GitHub Action, posts inline PR comments tagged with confidence tier.

Source of the strategic frame: [`docs/research/market-analysis-2026-05.md`](../research/market-analysis-2026-05.md).

## Why these three things together

- **Python alone unlocks ~30% of small teams.** TypeScript adds another ~25%. Rust-only is unsellable (~5% addressable). Without language coverage, no one can try mallard.
- **GitHub Action is the install path.** *"Anything over ~10 minutes of YAML wrangling loses the trial"* (devtoolsacademy, May 2026). Without it the language extractors are invisible.
- **Inline-comment posting with confidence-tier badges** is the positioning act. *"The AI reviewer that knows when it's guessing"* is unprovable until reviewers see `[ambiguous]` / `[extracted]` / `[structural-rule]` badges on real PRs.

Doing only one of the three buys nothing. Doing all three is the minimum viable trial flow.

## Sequencing rationale

| phase | duration | deliverable | ships independently? |
| --- | --- | --- | --- |
| **A. Python extractor** | weeks 1–3 | `mallard index` works on .py | yes — landable, smoke-validated on real PR |
| **B. TypeScript extractor** | weeks 4–5 | `mallard index` works on .ts / .tsx | yes — landable, smoke-validated on real PR |
| **C. GitHub Action + inline-comment posting** | weeks 6–7 | `mallard-review` action posts to GitHub PRs | yes — wraps Rust-only first if needed |
| **D. Polish + cost guardrails + README** | week 8 | shipping kit | yes |

Python first because the wedge dogfood (`docs/research/wedge-dogfood-1.md`) was Rust; we already have the Rust extractor as the reference shape. TypeScript second because tree-sitter-typescript ships two grammars (TS + TSX) — slightly more setup friction.

## Phase A — Python extractor (~3 weeks)

Reference: existing `RustExtractor` in `src/extractor.rs`. Same `SymbolExtractor` trait. Per ADR-0008 (heuristic name resolution), each language has its own constructor-filter + qualified-name heuristic.

### A1. Scaffolding + tree-sitter-python dep (~1 day)

- Add `tree-sitter-python = "0.23"` (or current) to `Cargo.toml`.
- Create `src/extractor_python.rs` mirroring `extractor.rs` shape; `impl SymbolExtractor for PythonExtractor`.
- Update `src/parsed_source.rs` and `FileProcessor` dispatch (the seam called out in CONTEXT.md) to pick `PythonExtractor` for `.py` files.
- Update CLI `--lang` allowlist and `language_allow_list` plumbing.
- Add `tests/fixtures/sample-python/` with a 3-file minimal fixture (`lib.py`, `app.py`, plus an `__init__.py`).
- **Smoke:** `mallard index --lang python tests/fixtures/sample-python` produces a non-empty index. New test `python_index_basic`.

**Done = symbols_in_file returns expected symbols for the smallest Python fixture.**

### A2. Python symbol patterns (~2 days)

tree-sitter-python patterns to capture:
- `function_definition name: (identifier)` — function or method
- `class_definition name: (identifier)` — class
- `assignment` at module top level where right-hand is non-trivial — covers Python "const" pattern
- `decorated_definition` — preserve decorators in signature? Just unwrap for now
- async fns: same node kind, mark in signature

Skip lambdas (per ADR-0008 lineage — bind to enclosing only, not as standalone symbols).

Per ADR-0010 confidence tier semantics, all definitions emit as `contains` edges with `Extracted`.

**Smoke:** symbol count on `tests/fixtures/sample-python/lib.py` matches hand-counted expected. New test `python_symbols_match_fixture`.

### A3. Python call extraction + Gap 2 / Gap 3 equivalents (~3 days)

Critical: the wedge-dogfood-1 gaps are not Rust-specific. Python has the same shapes:

- **Gap 2 equivalent — `self.attr.method()`.** Same bias problem. Distinguish bare `self.method()` (same class) from `self.<attr>.method()` (different type).
- **Gap 3 equivalent — calls inside macros.** Python doesn't have macros, but **decorators** + **comprehensions** + **f-strings** all carry expressions. f-string `{foo()}` is a real call site. tree-sitter parses f-strings into `formatted_value` nodes; check that calls inside resolve.
- **Same-name across methods** — fix C2 equivalent. A bare `name()` call cannot reach an instance method without a receiver. Filter out `Method` kind for bare-name candidates same as Rust.

Tree-sitter-python query patterns:
- `call function: (identifier) @ref.call.simple`
- `call function: (attribute attribute: (identifier) @ref.call.method)`
- `call function: (attribute object: (identifier) attribute: (identifier))` — for `module.fn()` pattern (treat as scoped)
- `import_statement` / `import_from_statement` for `imports`

Constructor-filter heuristic:
- Python convention: PascalCase = class. `Foo(x)` likely a constructor.
- Same `is_constructor_call` shape, language-specific names.

**Smoke:** new fixture `tests/fixtures/sample-python/wrapper.py` exercising:
- Inner / Outer class with shared method name (C4-equivalent)
- `self.inner.method()` (Gap 2 / C2)
- bare-name call to method-only short name (C2)
- f-string with embedded call (Gap 3 equivalent)

New integration tests mirroring the existing wrapper.rs test set.

**Done = all 5 regression patterns (Gap 2, C1, C2, C3, C4) have green tests in Python.**

### A4. Python qualified-name computation (~1 day)

Python qualified names follow dotted-module convention: `pkg.mod.Class.method`.

Strategy:
- For classes: `Class.method` (use class name, not file path)
- For module-level fns: `function_name` (bare)
- Skip dotted-module prefix at extractor layer — the resolver can join via file paths if needed (matches Rust's behavior where qualified names don't include module paths).

Document the choice in ADR-0012 (see below).

**Smoke:** `qualified_name` field on extracted symbols matches expectations for the fixture.

### A5. Python rules.yml + finding patterns (~1 day)

Port deterministic rules. Initial set:
- `python-bare-except` — `except:` without exception type
- `python-print-stdout` — `print()` calls (optional, off by default)
- `python-eval-use` — `eval(` or `exec(` calls
- `python-mutable-default` — `def f(x=[])` pattern

These are token-level rules already supported by mallard's existing rule infra; just add the YAML.

**Done = rules fixture finds expected violations.**

### A6. Wedge dogfood — real Python PR (~2 days)

Pick a recent merged PR from:
- `psf/requests` (HTTP, ~6k stars, slow merges so PRs are substantive)
- `tiangolo/fastapi` (async-heavy)
- `encode/httpx` (smaller scope)

Run base + head index + pr-review skill. Capture report at `docs/research/wedge-dogfood-python-1.md` — same shape as `wedge-dogfood-1.md`. Top friction goes to follow-up ADRs.

**Done = Python wedge report committed, listing the top 3–5 gaps surfaced.**

### Phase A acceptance

- All Python tests green
- `mallard index --lang python` works end-to-end on a 100+ file Python repo
- pr-review skill output cites Python symbol IDs correctly
- Wedge report shows mallard catches at least one real issue or surfaces meaningful structural evidence on the test PR

---

## Phase B — TypeScript extractor (~2 weeks)

Same shape as Phase A, abbreviated.

### B1. Scaffolding + tree-sitter-typescript dep (~1 day)

- `tree-sitter-typescript` ships **two grammars**: `language_typescript()` and `language_tsx()`. Both needed.
- Dispatch: `.ts` → TS grammar, `.tsx` → TSX grammar, `.d.ts` → skip (declarations only).
- Create `src/extractor_typescript.rs`.

### B2. TypeScript symbol patterns (~2 days)

- `function_declaration`, `function_signature`
- `class_declaration` + `method_definition`
- `interface_declaration`, `type_alias_declaration`
- Arrow functions assigned to const at module level: `lexical_declaration` with arrow_function on the right — extract as Function
- Object methods + property shorthand: skip for v1 (too lossy without type info)

### B3. TypeScript call extraction (~3 days)

- `call_expression function: (identifier)` — bare
- `call_expression function: (member_expression property: (property_identifier))` — method call
- Constructor calls: `new_expression` — emit constructor edge or filter? Decide: filter (matches Rust constructor-filter).
- `import_statement` for imports
- JSX: skip JSX element creation for v1 (too noisy; revisit if dogfood demands).

### B4. TypeScript qualified-name computation (~1 day)

- Class methods: `ClassName.method`
- Top-level: bare name
- Modules: skip; let resolver use file paths if needed.

### B5. TypeScript regression tests (~1 day)

Same shape as `wrapper.rs` / `wrapper.py` — Gap 2, C2, C4 equivalents.

### B6. Wedge dogfood — real TS PR (~2 days)

Candidates:
- `microsoft/vscode` (massive, slower merges)
- `axios/axios` (smaller, easier scope)
- `vercel/next.js` (volume, JSX-heavy — skip if JSX hurts)

Output: `docs/research/wedge-dogfood-ts-1.md`.

### Phase B acceptance

Same as Phase A, TS-side. Plus: TSX grammar handles a non-trivial .tsx file without parse errors blowing up.

---

## Phase C — GitHub Action + inline-comment posting (~2 weeks)

### C1. JSON output mode for pr-review (~3 days)

Currently the pr-review skill outputs Markdown for human consumption. Add a `--format=json` mode that emits:

```json
{
  "comments": [
    {
      "file": "src/foo.rs",
      "line_range": [42, 58],
      "symbol_qualified_name": "Foo::bar",
      "symbol_id": "abc123...",
      "confidence_tier": "extracted",
      "source_kind": "structural-rule" | "graph-synthesis",
      "rule_id": "rust-unsafe-block" | null,
      "body": "markdown text",
      "citations": [{"symbol_id": "...", "kind": "outbound_call"}]
    }
  ],
  "summary": {
    "symbols_changed": 12,
    "files_changed": 5,
    "comments_emitted": 7,
    "comments_dropped_to_budget": 3,
    "by_tier": { "extracted": 2, "inferred": 3, "ambiguous": 1, "structural-rule": 1 }
  }
}
```

New CLI command: `mallard pr-review --base-db <p> --head-db <p> --pr-source <ref> --format=json`.

Stage this so the skill can still produce markdown for local CLI use; JSON is for the action.

### C2. Inline comment posting via gh API (~2 days)

Map each comment's `(file, line_range)` to head-side line anchors. Use `gh api --method POST repos/{owner}/{repo}/pulls/{number}/reviews` with a single review batch containing all comments.

Body format:
```
[ambiguous · graph-synthesis] **Foo::bar's signature changed** — three callers still pass owned `Vec<EdgeKind>`.

Evidence: `Foo::bar` (head), inbound callers `Foo::run`, `build`.
```

Badge prefix is the trust-calibration positioning act. Reviewers see `[extracted]` vs `[ambiguous]` and can filter.

### C3. mallard-review GitHub Action (~3 days)

New repo `richard-gyiko/mallard-action`? Or `.github/actions/review/` inside mallard repo. Pick the in-repo composite action for now; spin out later.

Composite action steps:
1. Check out base + head SHAs (worktree).
2. Download pinned mallard binary from GH Releases.
3. Index base → `base.duckdb`, head → `head.duckdb`.
4. Run pr-review skill in JSON mode.
5. Post inline comments via `gh`.

Inputs:
- `anthropic-api-key` (required, secret)
- `languages-allow` (default: `rust,python,typescript`)
- `rules-path` (optional, default: skip)
- `max-comments` (default: 10)
- `min-confidence` (default: `inferred` — drops `ambiguous` and `unresolved` unless explicitly requested)
- `model` (default: `claude-sonnet-4-6`)

Quickstart YAML for users:
```yaml
name: mallard-review
on:
  pull_request:
    types: [opened, synchronize]
jobs:
  review:
    runs-on: ubuntu-latest
    permissions:
      pull-requests: write
      contents: read
    steps:
      - uses: richard-gyiko/mallard-action@v1
        with:
          anthropic-api-key: ${{ secrets.ANTHROPIC_API_KEY }}
```

5 lines. Hits the *"under 10 minutes setup"* bar.

### C4. Release-binary CI (~1 day)

GitHub Actions release workflow:
- Build mallard CLI for linux-x86_64, linux-arm64, macos-arm64, windows-x86_64.
- Tag-driven (`v0.x.y`).
- Publish to GH Releases as `mallard-{platform}.tar.gz`.

Action downloads the binary version pinned via the action's `package.json` or composite-action ref.

### C5. Dogfood action on mallard's own PRs (~1 day)

Enable `mallard-action` in `.github/workflows/review.yml` against mallard's own repo. Open synthetic PR with planted issues. Verify:
- Inline comments post against correct head-side lines
- Confidence badges render
- 5-minute setup time for a fresh user reproducible

### C6. README + cost note (~1 day)

README updates:
- "Get mallard reviewing your PRs in 5 minutes" quickstart
- Cost-per-PR table at Claude Sonnet 4.6 BYOK prices
- Confidence tier explainer with screenshots
- Comparison table vs CodeRabbit / Greptile / Copilot

### Phase C acceptance

- A fresh user can install mallard-action on a public repo, set their Anthropic key, and get inline review comments on their next PR — in under 10 minutes.
- Comments carry confidence-tier badges.
- $/PR is published, audited against a known PR set.

---

## Phase D — Polish (~1 week)

### D1. Cost guardrails (~2 days)

- `--max-tokens N` input on the action — cap LLM tokens per review.
- Smart context pruning: use confidence tiers to drop low-signal evidence from the prompt sent to Claude. Reduces cost; preserves recall (per ADR-0010, ambiguous edges go to reviewer, not to LLM prompt).
- Default budget: $0.30/PR target at Sonnet 4.6 + prompt caching. Document the math.

### D2. Comment-budget controls (~1 day)

- `max-comments` enforcement.
- `min-confidence` filter (`extracted` / `inferred` / `ambiguous` / `unresolved`).
- Auto-drop ambiguous-only comments unless reviewer explicitly opts in.

### D3. CONTEXT.md + system.md + README pass (~1 day)

Update:
- CONTEXT.md: 3 language extractors, FileProcessor dispatch by extension
- docs/system.md: action architecture, JSON output schema
- README: "production-ready for Python + TypeScript + Rust" headline
- next.md: clear stale items, mark Move 1 done

### D4. ADR-0012 for multi-language extractor architecture (~1 day)

Append-only ADR (per next.md note: "ADRs are append-only; new context = new ADR, not amendment"). Topics:
- Per-language `SymbolExtractor` trait split
- Constructor-filter heuristic is per-language; each language declares its own PascalCase / SCREAMING_SNAKE / etc. conventions
- Qualified-name strategy per language (Python dotted, TS module-flat, Rust impl-prefix)
- The wedge-dogfood-1 gaps (Gap 2 / C1 / C2 / C3 / C4 / C5 / C6) are reusable invariants — each new language extractor must satisfy them via its own fixture

### Phase D acceptance

- All docs in sync
- Cost guardrails enforced and tested
- ADR-0012 merged

---

## Risk register

| risk | mitigation |
| --- | --- |
| Python f-string call extraction is brittle | Treat f-strings like Rust macros — Gap-3-equivalent walker, escape hatch via `--no-fstring-descent` |
| tree-sitter-typescript dual grammar adds complexity | Ship TS-only in v1; TSX in v1.1 if dogfood demands |
| GitHub API rate limits on inline-comment batches | Single-review batching (one POST per PR); skip per-comment posts |
| Anthropic API cost explodes on large diffs | `max-tokens` cap; prompt caching; hard fail at $1/PR by default |
| 8-week window slips because of unknown unknowns | Phase A and Phase B ship independently; Phase C can wrap Rust-only as a fallback |

## Out of scope for Move 1

- Multi-repo or monorepo at scale (Move 3 territory — onboarding Q&A surface)
- MCP server wrapper (Move 3)
- SCIP export (Move 3)
- Custom LLM-soft rule YAML beyond deterministic rules (Move 2)
- Benchmark publication (Move 2)
- Self-hosted air-gapped binary tier (post-Move 3)

## Success criteria for Move 1

1. **A 5-person Python team can install mallard-action and get inline PR review in <10 minutes.**
2. **Same for a 5-person TypeScript team.**
3. **Reviewers can filter comments by confidence tier in the GitHub UI** (badge-based: `[extracted]` vs `[ambiguous]`).
4. **$/PR is published and audited at <$0.30 for a typical 200-LoC diff.**
5. **At least 10 external users install mallard-action within 4 weeks of release.**

If 1–4 land and 5 doesn't, the bottleneck is positioning (Move 2 — benchmark) not product. If 1–3 land and 4 explodes, cost guardrails need a follow-up sprint.

## What success unlocks

- Move 2 (benchmark) becomes credible — we can run mallard on a Python or TS PR corpus, not just Rust.
- Move 3 (Q&A / blast radius surfaces) becomes credible — same multi-language index drives multiple surfaces.
- First real revenue path opens — Team tier ($9/dev/mo) is sellable once a user has tried the GitHub Action.
