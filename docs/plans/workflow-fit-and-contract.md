# Mallard workflow fit + locked CLI contract

**Date:** 2026-05-25
**Distribution target:** [skills.sh](https://www.skills.sh/) (zero-gatekeeping Vercel Labs registry — `npx skills add <owner>/<repo>`)

---

## Current capabilities — what ships today

13 CLI surfaces under `mallard`:

| surface | what it returns | already-stable JSON? |
|---|---|---|
| `index --sha X --out DB <repo>` | per-SHA DuckDB index | n/a (side effect) |
| `query symbol <id>` | single symbol record | yes |
| `query neighbors <id> [--kind ...] [--direction ...]` | direct neighbors | yes |
| `query expand <id> --depth N` | bounded neighborhood (blast radius) | yes |
| `query findings [--rule ...] [--symbol-id ...]` | structural rule violations | yes |
| `query symbols-in-file <path>` | symbols defined in file | yes |
| `query edges-by-file <path>` | symbols + in/out edges per file | yes |
| `query unresolved-callers --name X[,Y]` | orphan callers after deletion | yes |
| `query importers-of <path>` | files importing path | yes |
| `query files [--prefix X]` | file lookup | yes |
| `query metadata` | index introspection | yes |
| `pr-review --base-db --head-db` | deterministic PR findings | yes |
| `diff-hunks --base --head` | git diff hunks JSON | yes |

**Implication:** Skill surface is mostly *renaming + composing existing primitives*, not building new ones. Low-risk, high-leverage.

---

## Real-world workflow fit

### Where mallard adds value TODAY (no new code)

| dev workflow | mallard surface | agent skill phrasing |
|---|---|---|
| **Pre-refactor scoping** — "rename `auth_check`, show callers" | `query expand <id> --depth 2 --direction inbound` | "use `mallard expand` to compute blast radius before refactoring" |
| **Post-deletion verification** — "did I miss callers after removing X?" | `query unresolved-callers --name X` | "after deleting public symbols, run `mallard unresolved-callers`" |
| **PR structural verification** — "what did this diff actually change?" | `pr-review --base-db --head-db --format json` | "for agent-authored PRs, run `mallard pr-review` to get cite-grounded structural deltas" |
| **File-level impact analysis** — "if I refactor this module, what depends on it?" | `query importers-of` + `query edges-by-file` | "use `mallard importers-of` before moving/renaming files" |
| **Onboarding Q&A** — "who calls foo?" | `query neighbors --direction inbound` | "answer 'who calls X' deterministically with citations" |
| **Code-review prep** — "summarize structural changes between two commits" | `pr-review` + `diff-hunks` | "structural summary of any base→head SHA pair" |
| **Dead-code scan** — "any unused public symbols in this module?" | `query symbols-in-file` + `query neighbors` per symbol | "find dead symbols via inbound-neighbor count = 0" |

### Where mallard has GAPS (extensions worth shipping)

| gap | impact on skill UX | proposed extension | effort |
|---|---|---|---|
| No symbol lookup by qualified name | Agent must do 2-step: `find-by-name` → `expand <id>`. Awkward in skill body | **`query find --qname X`** (backlog item lifted to first-class) | ~1 day |
| No composite "blast-radius" command | Agent must call `query symbol` to get ID, then `query expand`. Two round-trips | **`query blast-radius --qname X`** — composite returning {symbol, callers, callees, test seams, confidence} | ~1 day |
| No standalone "test seams for X" query | pr-review computes seams but no standalone lookup | **`query test-seams --qname X`** | ~half day |
| No symbol-level diff between indexes | pr-review returns full findings; no light "what symbols changed" primitive | **`query symbol-diff --base --head`** | ~1 day |
| No JSON `schema_version` field on outputs | Skill body can't guarantee parse stability across mallard versions | Add `schema_version: "1.0"` field to every `--format json` output | ~half day |
| `--format json` not uniformly default | Some commands JSON, some markdown. Inconsistent | Make `json` default for all query subcommands; keep `markdown` as opt-in | ~half day |
| No incremental indexing | Full rebuild per SHA → slow on monorepos → skill feels heavy | Cache + incremental delta (backlog) | ~1 week — defer |
| Indexing not exposed as skill action | Agent has to manually shell out to `mallard index` first | Document index-on-demand pattern in SKILL.md body | docs only |

**Decision:** ship the first 4 extensions before locking contract. ~3 days total. Defer incremental indexing.

---

## Locked CLI JSON contract — v1.0

Skill surface = 3 commands. Other 10 remain available as power-user primitives but skill body documents only these three.

### `mallard blast-radius --qname X --index DB --format json` (NEW)

```jsonc
{
  "schema_version": "1.0",
  "qname": "module.auth.check_token",
  "symbol_id": "abc123def456",
  "kind": "function",
  "callers": [
    { "file": "api/handlers.py", "line": 42, "qname": "module.api.login", "confidence": "structural-rule" }
  ],
  "callees": [
    { "file": "lib/jwt.py", "line": 18, "qname": "module.lib.decode_jwt", "confidence": "structural-rule" }
  ],
  "test_seams": [
    { "file": "tests/test_auth.py", "line": 12, "qname": "test_check_token_valid" }
  ],
  "confidence": "structural-rule"
}
```

Backed by composite of `query symbol` (qname → id) + `query expand --depth 1 --direction both` + test-seam classifier.

### `mallard verify-diff --base-db DB1 --head-db DB2 --format json` (RENAME of `pr-review`)

```jsonc
{
  "schema_version": "1.0",
  "base_sha": "abc...",
  "head_sha": "def...",
  "findings": [
    {
      "tier": "structural-rule",
      "kind": "caller-drop",
      "msg": "Removed `auth_check`; still called at api/handlers.py:42",
      "citation": { "symbol_id": "abc123", "file": "api/handlers.py", "line": 42 }
    }
  ],
  "stats": { "useful_tiers_emitted": 18, "ambiguous": 4, "wrong": 0 }
}
```

Keep `pr-review` as alias for backward compat.

### `mallard find --qname X --index DB --format json` (NEW)

```jsonc
{
  "schema_version": "1.0",
  "qname": "module.auth.check_token",
  "matches": [
    { "symbol_id": "abc123", "file": "lib/auth.py", "line": 8, "kind": "function" }
  ]
}
```

---

## Power-user surface (not in SKILL.md but documented for tool builders)

All 10 existing `query` subcommands stay. JSON contract v1.0 applies to all (add `schema_version` field). Skill body links to `docs/cli-json-contract.md` for full reference.

This means **mallard remains a UNIX-composable CLI** — agents can chain power-user commands when blast-radius isn't enough.

---

## skills.sh distribution path

Install via [skills.sh](https://www.skills.sh) — Vercel Labs' decentralised Agent Skills registry. Zero submission form.

```bash
npx skills add richard-gyiko/mallard
```

The CLI auto-discovers `skills/mallard/SKILL.md` in the public repo and installs it locally for any agent that honors the [Anthropic Agent Skills spec](https://github.com/anthropics/skills) — Claude Code, Codex CLI, ChatGPT.

The actual shipped frontmatter and body live at [`skills/mallard/SKILL.md`](../../skills/mallard/SKILL.md).

---

## Mallard vs LSP — honest comparison

LSP (rust-analyzer, pyright, tsserver) is the established way to query code intelligence. Mallard overlaps and differs. Don't pretend mallard subsumes LSP.

### Where LSP wins

| capability | LSP | mallard |
|---|---|---|
| Within-language semantic accuracy (type inference, generics, trait dispatch) | ✔ | ✘ tree-sitter parse — structural only, no type system |
| Refactoring primitives (rename, extract, inline) | ✔ | ✘ read-only |
| Real-time editor experience | ✔ | ✘ batch CLI |
| Hover docs / completions | ✔ | ✘ not the product |
| Per-language standard, every editor supports | ✔ | ✘ not standardized |
| Disambiguation via type system | ✔ | partial (qname + structural rules) |

If user wants "rename safely in my IDE while typing" → use LSP.

### Where mallard wins

| capability | LSP | mallard |
|---|---|---|
| Per-SHA reproducible snapshot | ✘ live-only, no historical SHA | ✔ DuckDB file per commit |
| Diff-aware structural deltas (base SHA → head SHA) | ✘ LSP has no diff concept | ✔ `verify-diff` |
| Cross-language unified graph | ✘ per-language server | ✔ Python imports TS file? one query |
| Headless / CI / one-shot binary | ✘ needs running server | ✔ single Rust binary, no daemon |
| Citation discipline (audit-grade anchors) | ✘ editor positions decay | ✔ stable symbol_id + file:line |
| Confidence tiers on resolution | ✘ accurate-or-error | ✔ 4-tier model |
| Bulk graph SQL queries | ✘ one-symbol-at-a-time | ✔ DuckDB `WHERE` over whole graph |
| Portable index artifact | ✘ in-memory per-server | ✔ DuckDB file ships/diffs/caches |
| Works in GitHub Actions without LSP setup | ✘ heavy per-language install | ✔ single binary |

### Honest tradeoff

LSP = **live, deep within-language, editor-bound** — for human-in-editor real-time.
Mallard = **batch, shallow cross-language, snapshot-native** — for *agent-in-CI verifying diffs across SHAs*.

Different products, different problems. **Use both.** Agents use LSP for live editing; mallard for cross-SHA diff verification, blast-radius scoping, CI-time structural audits.

### Could mallard adopt LSP under hood?

Serena MCP does this. Buys type-accurate caller graph. Costs: ~6 months engineering, lose single-binary distribution, lose cross-language unified graph, lose CI velocity (LSP startup time per language).

**Decision:** Don't adopt LSP. Stay tree-sitter + DuckDB. Win on diff/SHA/cross-language/headless. Concede within-language semantic depth.

### Competitive threats from LSP-based products

- **Serena MCP (24k★)** — wraps LSP for symbol-nav. If they add diff-awareness + per-SHA caching → close mallard's gap. Window ~12 months.
- **Sourcegraph SCIP** — precomputes LSP-grade indexes per SHA. Closest in spirit. Differentiator: SCIP is enterprise-on-prem-complex; mallard is single-binary OSS.
- **GitHub Stack Graphs / tree-sitter-graph** — GitHub could ship this free. Window ~18-24 months.

---

## What this DOESN'T extend (deliberate)

- **No new languages** (Go/Java/Ruby/C#). Python+TS+Rust+JS already cover ~80% of agent traffic per State-of-AI-PRs research.
- **No MCP wrapper.** CLI + Skill is consensus distribution mid-2026 ([Scalekit](https://www.scalekit.com/blog/mcp-vs-cli-use), [Apideck](https://www.apideck.com/blog/mcp-server-eating-context-window-cli-alternative)).
- **No LLM synthesis.** Negates moat. Mallard's pitch: "we don't hallucinate."
- **No semantic / type-aware analysis.** Stay structural. Adding type inference = ~6-12 months work, kills shipping velocity.
- **No cross-repo / monorepo dep graph.** Single-repo scope today. Revisit if user demand warrants.
