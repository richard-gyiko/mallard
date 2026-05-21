---
name: pr-review
description: Review a pull request against this repository by composing the `mallard` skill, `gh`, `git`, and `jq` into a layered pipeline (selective retrieval → deterministic findings → LLM synthesis → severity-calibrated output). Use when the user asks to "review PR #N", "check what this diff might break", "find affected callers of changed symbols", or any PR-review-shaped task that needs structural evidence rather than free-form opinion. Output is a markdown list of comments, each citing the symbol IDs / edge paths / rule IDs it depends on. First-pass assistant with human oversight per ADR-0009; emitted comments do not auto-block merges. Skip when the diff is docs-only / formatting-only / config-only — no structural evidence to assemble.
---

# pr-review

Layered-pipeline PR reviewer per [ADR-0009](../../../docs/decisions/0009-pr-review-architecture-pattern.md). Composes the [mallard skill](../mallard/SKILL.md) (`mallard index` + `mallard query *`) with `gh`, `git`, `jq` and your own LLM synthesis.

Background:
- [docs/specs/pr-review/pull-request-review.md](../../../docs/specs/pr-review/pull-request-review.md) — wedge contract (citations required, deterministic vs synthesized labelling, comment budget).
- [ADR-0009](../../../docs/decisions/0009-pr-review-architecture-pattern.md) — pipeline pattern + first-pass-assistant framing.
- [mallard skill](../mallard/SKILL.md) — index + query primitives, JSON envelope, composition patterns, invocation modes.

## Prereqs

- `gh` (authenticated via `gh auth login`) — for PR metadata. If unavailable, accept base + head SHAs as direct args.
- `git` — for worktrees.
- `cargo` or a prebuilt mallard binary. **Strong recommendation**: `cargo build --release` once at session start and call `./target/release/mallard` directly. Calling `cargo run -- query ... > file.json` is fragile because cargo writes incremental-compilation warnings to stderr that bleed into stdout under some shells.
- `jq` for JSON extraction (Bash) **or** PowerShell `ConvertFrom-Json` (Windows). The recipes below default to `jq`; PowerShell equivalents in [`references/powershell-recipes.md`](references/powershell-recipes.md).
- On Windows the release binary may fail under git-bash (`STATUS_DLL_NOT_FOUND`); use PowerShell or cmd instead.

## Trust framing

This is a **first-pass assistant with human oversight**. Deterministic-rule findings *may* gate merges (they are reproducible for a given SHA + rule-set hash); LLM-synthesized comments **never** auto-block. Always surface the source kind on every comment.

## Workflow

Seven stages. Each stage runs to completion before the next starts.

### 1. Identify base + head SHAs and changed files

```bash
PR=<n>
gh pr view "$PR" --json baseRefOid,headRefOid,files \
  | jq -r '"BASE=\(.baseRefOid)\nHEAD=\(.headRefOid)"'

CHANGED=$(gh pr view "$PR" --json files | jq -r '.files[].path')
```

If the user gives base + head SHAs directly instead of a PR number, skip the `gh` call and use what they gave.

Filter `$CHANGED` to files mallard can index (today: `*.rs` only). If nothing remains, stop and emit a one-line "no structural evidence — docs / config / non-Rust diff" summary.

### 2. Materialize two indexes

One per SHA, using a git worktree so the working tree is undisturbed.

```bash
MALLARD=./target/release/mallard   # built once at session start; or use `cargo run --quiet --` and redirect stderr on every call

git worktree add /tmp/pr-base "$BASE"
"$MALLARD" index /tmp/pr-base --sha "$BASE" --out base.duckdb --rules tests/fixtures/rules.yml
git worktree remove /tmp/pr-base

git worktree add /tmp/pr-head "$HEAD"
"$MALLARD" index /tmp/pr-head --sha "$HEAD" --out head.duckdb --rules tests/fixtures/rules.yml
git worktree remove /tmp/pr-head
```

Pass `--rules` only if the repo has a rules YAML; otherwise findings will be empty (still valid, just no deterministic-hard layer).

**Caching**: if you're reviewing multiple PRs against the same base SHA, reuse `base.duckdb` across reviews. The mallard CLI doesn't need re-running for an unchanged base.

### 3. Compute the changed-symbol set (signature-shape diff)

For each changed file, set-difference head symbol IDs against base. Per mallard's stable-ID rule (`(file_path, qualified_name, kind, signature)`), this yields:

- **added**: in head, not in base.
- **removed**: in base, not in head.
- **modified-signature**: same path + qualified name + kind, different signature → looks like one removed + one added with the same name. Reconcile by qualified name.

```bash
for f in $CHANGED; do
  comm -23 \
    <("$MALLARD" query symbols-in-file "$f" --index head.duckdb 2>/dev/null | jq -r '.value[].id' | sort) \
    <("$MALLARD" query symbols-in-file "$f" --index base.duckdb 2>/dev/null | jq -r '.value[].id' | sort) \
    | while read -r id; do echo "added $f $id"; done
done > added.txt
```

(Mirror with `-13` for removed.) For "modified-signature" reconciliation: group added+removed pairs in the same file with the same `qualified_name`; treat as modified-signature rather than separate add/remove.

### 4. Compute body-level edge diff (modified-body)

Stage 3 misses the bulk of real PRs: **body-only changes don't change the symbol ID.** A function whose body changes but signature stays stable looks unchanged in stage 3. Edge-diff catches it.

Use the bulk `edges-by-file` primitive: one query per file per direction returns every symbol in the file plus its outbound + inbound edges. Set-diff base vs head on each shared-ID symbol's `outbound` calls.

```bash
for f in $CHANGED; do
  base_bundles=$("$MALLARD" query edges-by-file "$f" --kind calls --direction out --index base.duckdb 2>/dev/null \
                 | jq -c '.value[]')
  head_bundles=$("$MALLARD" query edges-by-file "$f" --kind calls --direction out --index head.duckdb 2>/dev/null \
                 | jq -c '.value[]')

  # Index base bundles by symbol_id for fast lookup.
  declare -A base_by_id
  while IFS= read -r b; do
    id=$(echo "$b" | jq -r '.symbol.id')
    base_by_id["$id"]="$b"
  done <<< "$base_bundles"

  while IFS= read -r hb; do
    id=$(echo "$hb" | jq -r '.symbol.id')
    [ -z "${base_by_id[$id]+x}" ] && continue   # added in head only -> stage 3 already saw it
    bb="${base_by_id[$id]}"
    base_t=$(echo "$bb" | jq -r '.outbound[] | .dst.qualified_name // ("[" + .dst_unresolved + "]")' | sort -u)
    head_t=$(echo "$hb" | jq -r '.outbound[] | .dst.qualified_name // ("[" + .dst_unresolved + "]")' | sort -u)
    added=$(comm -13 <(echo "$base_t") <(echo "$head_t"))
    removed=$(comm -23 <(echo "$base_t") <(echo "$head_t"))
    if [ -n "$added" ] || [ -n "$removed" ]; then
      qname=$(echo "$hb" | jq -r '.symbol.qualified_name')
      echo "modified-body $f $qname id=$id added=[$(echo "$added" | tr '\n' ',')] removed=[$(echo "$removed" | tr '\n' ',')]"
    fi
  done <<< "$head_bundles"
done > modified_body.txt
```

A symbol with `modified-body` status has a changed implementation. The added/removed callee names are the **shape of what changed**, even if the body source isn't available to mallard.

**Performance**: `edges-by-file` collapses what was previously N+1 (one `neighbors` per symbol) into one query per file per direction. On a 4-file PR with ~60 stable symbols, stage 4 runs in seconds rather than minutes.

If both stage 3 and stage 4 produce empty sets, stop and emit "diff touches no parseable structural changes" summary.

### 5. Gather evidence per changed symbol

For each `(kind, file, id)` in the changed set (added, modified-signature, modified-body — all the same evidence shape; removed get base-side evidence instead):

```bash
"$MALLARD" query expand "$id" --depth 1 --kind calls --direction both --index head.duckdb > "evidence/$id.expand.json"
"$MALLARD" query findings --symbol-id "$id" --index head.duckdb > "evidence/$id.findings.json"
```

Depth-1 is the right default. Go to depth-2 only when the symbol is small (a leaf function) and the depth-1 frontier is sparse. Higher depths blow the context budget per ADR-0007's attention-dilution evidence.

For removed symbols, evidence comes from `base.duckdb` instead of `head.duckdb` — use it to reason about callers that no longer have a target.

### 6. Synthesize review comments

For each changed symbol, decide whether to emit comments. Two channels:

**Deterministic-hard** — every finding from `evidence/<id>.findings.json` becomes a candidate comment. Body = the rule's `message`. Source kind = `structural-rule`. Confidence = high (rules are reproducible).

**LLM-soft** — reason from the assembled evidence (including the stage-4 modified-body deltas) to identify likely issues. Examples worth surfacing:

- A modified-signature function whose callers (`expand --direction in --kind calls`) pass arguments that no longer match the new signature.
- A modified-body function whose new outbound calls land on `dst_unresolved` names that look risky (e.g., suddenly calling `panic!`, `unwrap`, `unsafe` blocks).
- A removed public function with inbound callers in the head index (definitely broken).
- A new function added but never called (possibly dead code — low confidence).
- A function that grew significantly and now hits a structural-rule finding.

For every emitted comment, the spec ([docs/specs/pr-review/pull-request-review.md](../../../docs/specs/pr-review/pull-request-review.md)) requires citing the evidence: at minimum a symbol ID, optionally edge paths and rule IDs. Comments without citations must not be emitted.

### 7. Cap and label

Per-PR comment budget: default 10 total comments (5 deterministic-hard + 5 LLM-soft is a starting split; adjust per repo). Drop lowest-confidence comments first if over budget.

## Output

Markdown ordered by file path, head-side line range. Each comment looks like:

```
### src/query.rs:312–340 — `IndexReader::expand` (modified-signature)

`expand`'s signature changed from `(id, depth, kinds, dir)` to `(id, depth, &kinds, dir)`. Three caller sites still pass an owned `Vec<EdgeKind>` and will hit a borrow-checker error.

- **source**: graph-synthesis
- **confidence**: high
- **evidence**:
  - symbol: `9fe59e2a4b882d4b16292ebba34fcef5` (IndexReader::expand, head.duckdb)
  - inbound callers: `0cea32a34b4063...` (IndexReader::run), `746a357a24cb...` (build)
```

End the report with a summary line:

```
Reviewed N changed symbols across M files (X added, Y modified-signature, Z modified-body, W removed). Emitted P comments (Q structural-rule, R graph-synthesis). Dropped S to fit budget.
```

## Gotchas

- **Body-only changes need stage 4.** The most common PR shape (refactor, internal logic tweak) doesn't change any signature. Skip stage 4 and you'll emit "diff touches no symbols" on real PRs.
- **Const / static / type-alias symbols carry no `calls` edges.** They aren't callable, so `expand --kind calls` returns empty. That's expected — don't read it as "dead code". See `mallard` skill's Gotchas.
- **Cross-file resolution is heuristic** ([ADR-0008](../../../docs/decisions/0008-heuristic-name-resolution.md)). Stdlib / external calls land as `dst_unresolved`. Synthesized comments must not assume "no caller" means "no caller exists" — only "no caller could be resolved within the indexed crate".
- **Worktrees use disk and time.** Two `git worktree add` + `mallard index` runs is the right cost for a real PR review. Cache `base.duckdb` per base SHA across reviews of the same branch.
- **`comm` requires sorted input.** Always pipe `| sort` before `comm`.
- **Unparseable files** (per mallard's `parse_errors` table) still appear in `symbols-in-file` as `[]`. Don't conclude "no symbols changed" from an empty `comm` if the file failed to parse — check the parse-error log first.
- **Generated files and lockfiles** should be suppressed before stage 1. Surface them as "skipped" in the summary, never as findings.
- **`gh pr view`** requires `gh auth login`. If the user can't authenticate, accept base + head SHAs as direct arguments.
- **Cargo stderr corrupts captured JSON.** `cargo run -- query ... > file.json` writes incremental-compile warnings into `file.json`. Always either use the prebuilt binary or redirect stderr (`2>/dev/null` in bash, `2>$null` in PowerShell).
- **PowerShell users**: see [`references/powershell-recipes.md`](references/powershell-recipes.md) for the stage-3 / stage-4 equivalents without `jq` and `comm`.

## When this skill is wrong

- The user wants a code-style review (run a formatter or linter directly, not mallard).
- The PR is docs-only, config-only, or in a language mallard doesn't index. Emit one-line "no structural evidence" summary and stop.
- The user wants to *generate the patch*, not review it. Different task.
