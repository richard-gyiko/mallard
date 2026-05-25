---
name: mallard
description: Query a mallard structural code-index for caller/callee graph, blast-radius, test-seam discovery, and cross-SHA symbol diff. Use BEFORE renaming, removing, or modifying a public symbol to scope refactor impact; AFTER an AI agent edits code to verify what structurally changed; whenever the user asks "who calls X", "what breaks if I rename Y", "what's the blast radius of Z", "find symbol foo", "what tests exercise X", "what symbols changed between these two SHAs", "verify this diff", or "show me the impact of this refactor". Mallard returns deterministic, citation-grounded answers from a per-SHA DuckDB index — no LLM, no hallucination, every result anchored to a symbol ID + file:line. Covers Rust, Python, TypeScript, JavaScript. Not an LSP — answers cross-SHA diff questions LSPs cannot.
allowed-tools: [Bash, Read]
---

# Mallard — structural code-intelligence

Mallard is a CLI returning JSON over stdout. Invoke via `Bash`. Output is deterministic and citation-grounded.

## Prerequisite — build an index first

Every query targets a built index (DuckDB file). Build once per commit SHA:

```bash
mallard index --sha "$(git rev-parse HEAD)" --out .mallard/head.duckdb .
```

Indexing takes ~10s per 100kloc. The `.duckdb` file is reproducible from the SHA. Re-build only when source changes.

If the user asks any of the questions in the description without an existing index, build one first. Default output: `.mallard/head.duckdb`.

## Four agent-facing commands

All four emit `schema_version: "1.0"`. Always check it before parsing.

### 1. `find` — qname lookup

```bash
mallard query find --index .mallard/head.duckdb --qname auth_check
```

Returns symbols whose `qualified_name` equals (or suffix-matches) `auth_check`. Exact matches rank first. Use when the user gives a short name and you need the full symbol record (file, line, kind, signature).

### 2. `blast-radius` — composite impact

```bash
mallard query blast-radius --index .mallard/head.duckdb --qname auth_check
```

Returns `{symbol, callers, callees, test_seams, other_qname_matches}`. **Call this BEFORE proposing a rename, removal, or signature change.** It shows every site that breaks.

- `callers` — inbound edges (who calls this symbol)
- `callees` — outbound edges (what this symbol calls)
- `test_seams` — subset of callers from test files
- `other_qname_matches` — sibling symbols matching the qname (disambiguate via `path` + `kind`)

`value: null` if qname has no match.

### 3. `test-seams` — which tests exercise a symbol

```bash
mallard query test-seams --index .mallard/head.duckdb --qname auth_check
```

Returns just the test seams from blast-radius. Use to scope which tests to run after modifying a symbol.

### 4. `symbol-diff` — what changed between two SHAs

Needs two indexes — base and head. **`mallard index` reads the working tree, not the SHA's actual file content.** To compare real SHAs, check out each one (or use a `git worktree`) before indexing:

```bash
git checkout "$BASE_SHA"
mallard index --sha "$BASE_SHA" --out .mallard/base.duckdb .

git checkout "$HEAD_SHA"
mallard index --sha "$HEAD_SHA" --out .mallard/head.duckdb .

mallard symbol-diff --base-db .mallard/base.duckdb --head-db .mallard/head.duckdb
```

Or use worktrees to avoid mutating the user's checkout:

```bash
git worktree add /tmp/base "$BASE_SHA"
mallard index --sha "$BASE_SHA" --out .mallard/base.duckdb /tmp/base
git worktree remove /tmp/base
```

Returns `{added, removed, modified}`. Symbols match by `(qualified_name, path, signature)`. `modified` = same key, different anchor (body changed). Use to verify what an AI agent actually changed structurally — particularly for agent-authored PRs.

## Composition patterns

### Pre-refactor scoping

```bash
mallard query blast-radius --index .mallard/head.duckdb --qname process_request \
  | jq '.value.callers[] | "\(.path):\(.anchor.start_line) \(.qualified_name)"'
```

### Post-deletion verification

After agent removes a function, check for orphan callers:

```bash
mallard query unresolved-callers --index .mallard/head.duckdb --name deleted_fn
```

### Cross-SHA verification (agent PRs)

```bash
mallard symbol-diff --base-db .mallard/base.duckdb --head-db .mallard/head.duckdb \
  | jq '.removed[] | "\(.path):\(.anchor.start_line) \(.qualified_name)"'
```

Then for each removed symbol, check unresolved callers in HEAD to catch missed updates.

## When to refuse

- User asks about runtime behavior, types, generics resolution → mallard sees structure only. Use LSP / rust-analyzer / pyright.
- User asks about security patterns or CVEs → mallard is not SAST. Use Semgrep / CodeQL.
- User asks about a language outside Rust / Python / TypeScript / JavaScript → unsupported. Say so.
- Index not built and user is in read-only mode → ask before building.

## Output format guarantees

| field | guarantee |
|---|---|
| `schema_version` | always `"1.0"` on the 4 commands above. Refuse to parse other values |
| `kind` (query results) | tagged enum discriminator |
| `value` | empty array on no match, NOT null. Exception: `blast_radius.value` is null when qname unmatched |
| exit code | 0 = success with JSON on stdout; non-zero = error on stderr, no JSON |
| citations | every `SymbolRecord` includes stable `id` (content hash) + `path` + `anchor.start_line` |

Full schema reference: `docs/cli-json-contract.md` in the mallard repo.

## Power-user surface (unversioned)

These commands exist but DON'T carry `schema_version`. Use only when the 4 versioned commands above don't fit:

`query symbol`, `query neighbors`, `query expand`, `query findings`, `query symbols-in-file`, `query edges-by-file`, `query unresolved-callers`, `query importers-of`, `query files`, `query metadata`, `pr-review`, `diff-hunks`.

Their shapes are documented in `docs/cli-json-contract.md` but may evolve via SemVer of the binary.

## Cardinal rules

1. **Never invent symbol names or file paths.** If `find` or `blast-radius` returns empty, the symbol doesn't exist in the index. Tell the user. Don't guess.
2. **Cite every claim.** When reporting findings to the user, include `path:line` from the mallard output. The user must be able to verify.
3. **Build once, query many.** Don't re-index between queries unless source changed.
4. **Prefer `blast-radius` over chaining `find` + `expand`** — same result, one shell call, smaller token surface.
