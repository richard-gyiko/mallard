---
name: pr-review
description: Review a pull request against this repository by composing the `mallard` skill, `gh`, `git`, and `jq` into a layered pipeline (selective retrieval → deterministic findings → LLM synthesis → severity-calibrated output). Use when the user asks to "review PR #N", "check what this diff might break", "find affected callers of changed symbols", or any PR-review-shaped task that needs structural evidence rather than free-form opinion. Output is a markdown list of comments, each citing the symbol IDs / edge paths / rule IDs it depends on. First-pass assistant with human oversight per ADR-0009; emitted comments do not auto-block merges. Skip when the diff is docs-only / formatting-only / config-only — no structural evidence to assemble.
---

# pr-review

Layered-pipeline PR reviewer per [ADR-0009](../../../docs/decisions/0009-pr-review-architecture-pattern.md). Composes the [mallard skill](../mallard/SKILL.md) (`mallard index` + `mallard query *`) with `gh`, `git`, `jq` and your own LLM synthesis.

Background:
- [docs/specs/pr-review/pull-request-review.md](../../../docs/specs/pr-review/pull-request-review.md) — wedge contract (citations required, deterministic vs synthesized labelling, comment budget).
- [ADR-0009](../../../docs/decisions/0009-pr-review-architecture-pattern.md) — pipeline pattern + first-pass-assistant framing.
- [mallard skill](../mallard/SKILL.md) — index + query primitives, JSON envelope, composition patterns.

## Trust framing

This is a **first-pass assistant with human oversight**. Deterministic-rule findings *may* gate merges (they are reproducible for a given SHA + rule-set hash); LLM-synthesized comments **never** auto-block. Always surface the source kind on every comment.

## Workflow

Six stages. Each stage runs to completion before the next starts.

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
git worktree add /tmp/pr-base "$BASE"
mallard index /tmp/pr-base --sha "$BASE" --out base.duckdb --rules tests/fixtures/rules.yml
git worktree remove /tmp/pr-base

git worktree add /tmp/pr-head "$HEAD"
mallard index /tmp/pr-head --sha "$HEAD" --out head.duckdb --rules tests/fixtures/rules.yml
git worktree remove /tmp/pr-head
```

Pass `--rules` only if the repo has a rules YAML; otherwise findings will be empty (still valid, just no deterministic-hard layer).

### 3. Compute the changed-symbol set

For each changed file, set-difference head symbol IDs against base. Per mallard's stable-ID rule (`(file_path, qualified_name, kind, signature)`), this yields:

- **added**: in head, not in base.
- **removed**: in base, not in head.
- **modified**: same path + qualified name + kind, different signature → looks like one removed + one added with the same name. Reconcile by qualified name.

```bash
for f in $CHANGED; do
  comm -23 \
    <(mallard query symbols-in-file "$f" --index head.duckdb 2>/dev/null | jq -r '.value[].id' | sort) \
    <(mallard query symbols-in-file "$f" --index base.duckdb 2>/dev/null | jq -r '.value[].id' | sort) \
    | while read -r id; do echo "added $f $id"; done
done > added.txt
```

(Mirror with `-13` for removed.) For "modified" reconciliation: group added+removed pairs in the same file with the same `qualified_name`; treat as modified rather than separate add/remove.

If the changed-symbol set is empty after filtering, stop and emit "diff touches no parseable symbols" summary.

### 4. Gather evidence per changed symbol

For each `(kind, file, id)` in the changed set (treat added and modified the same way; removed get the base-side evidence instead):

```bash
mallard query expand "$id" --depth 1 --kind calls --direction both --index head.duckdb > "evidence/$id.expand.json"
mallard query findings --symbol-id "$id" --index head.duckdb > "evidence/$id.findings.json"
```

Depth-1 is the right default. Go to depth-2 only when the symbol is small (a leaf function) and the depth-1 frontier is sparse. Higher depths blow the context budget per ADR-0007's attention-dilution evidence.

For removed symbols, evidence comes from `base.duckdb` instead of `head.duckdb`. Use it to reason about callers that no longer have a target.

### 5. Synthesize review comments

For each changed symbol, decide whether to emit comments. Two channels:

**Deterministic-hard** — every finding from `evidence/<id>.findings.json` becomes a candidate comment. Body = the rule's `message`. Source kind = `structural-rule`. Confidence = high (rules are reproducible).

**LLM-soft** — reason from the assembled evidence to identify likely issues. Examples worth surfacing:

- A modified function whose callers (`expand --direction in --kind calls`) pass arguments that no longer match the new signature.
- A removed public function with inbound callers in the head index (definitely broken).
- A new function added but never called (possibly dead code — low confidence).
- A function that grew significantly and now hits a structural-rule finding.

For every emitted comment, the spec ([docs/specs/pr-review/pull-request-review.md](../../../docs/specs/pr-review/pull-request-review.md)) requires citing the evidence: at minimum a symbol ID, optionally edge paths and rule IDs. Comments without citations must not be emitted.

### 6. Cap and label

Per-PR comment budget: default 10 total comments (5 deterministic-hard + 5 LLM-soft is a starting split; adjust per repo). Drop lowest-confidence comments first if over budget.

## Output

Markdown ordered by file path, head-side line range. Each comment looks like:

```
### src/query.rs:312–340 — `IndexReader::expand` (modified)

`expand`'s signature changed from `(id, depth, kinds, dir)` to `(id, depth, &kinds, dir)`. Three caller sites still pass an owned `Vec<EdgeKind>` and will hit a borrow-checker error.

- **source**: graph-synthesis
- **confidence**: high
- **evidence**:
  - symbol: `9fe59e2a4b882d4b16292ebba34fcef5` (IndexReader::expand, head.duckdb)
  - inbound callers: `0cea32a34b4063...` (IndexReader::run), `746a357a24cb...` (build)
```

End the report with a summary line:

```
Reviewed N changed symbols across M files. Emitted X comments (Y structural-rule, Z graph-synthesis). Dropped W to fit budget.
```

## Gotchas

- **Worktrees use disk and time.** Two `git worktree add` + `mallard index` runs is the right cost for a real PR review. Cache `base.duckdb` per base SHA across reviews of the same branch.
- **`comm` requires sorted input.** Always pipe `| sort` before `comm`.
- **Unparseable files** (per mallard's `parse_errors` table) still appear in `symbols-in-file` as `[]`. Don't conclude "no symbols changed" from an empty `comm` if the file failed to parse — check the parse-error log first.
- **Cross-file resolution is heuristic** ([ADR-0008](../../../docs/decisions/0008-heuristic-name-resolution.md)). Synthesized comments should not assume an absent caller means no caller — only that no caller could be resolved within the repo. Stdlib / external calls are invisible.
- **Generated files and lockfiles** should be suppressed before stage 1. Surface them as "skipped" in the summary, never as findings.
- **`gh pr view`** requires `gh auth login`. If the user can't authenticate, accept base + head SHAs as direct arguments.

## When this skill is wrong

- The user wants a code-style review (run a formatter or linter directly, not mallard).
- The PR is docs-only, config-only, or in a language mallard doesn't index. Emit one-line "no structural evidence" summary and stop.
- The user wants to *generate the patch*, not review it. Different task.
