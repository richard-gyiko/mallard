# PR review recipes

Multi-step shell compositions for PR-review-shaped tasks. Read this when assembling evidence for an LLM reviewer over a real diff.

See [ADR-0009](../../../../docs/decisions/0009-pr-review-architecture-pattern.md) for the layered pipeline these recipes compose into: selective retrieval → deterministic analyzers → LLM synthesis → severity-calibrated output → project memory.

## Walk a diff's blast radius (single-index, simple case)

Given a list of changed file paths and one index for the head commit, list inbound callers for every symbol the diff touches.

```bash
git diff --name-only main HEAD | while read -r f; do
  mallard query symbols-in-file "$f" --index "$INDEX" \
    | jq -r '.value[].id' \
    | while read -r id; do
        mallard query neighbors "$id" --kind calls --direction in --index "$INDEX"
      done
done | jq -s 'add | .value // .'
```

## PR-review chain (base + head indexes)

Two-index workflow that approximates symbol-diff against full indexes. Each step is one shell command; the LLM composes them.

```bash
# 1. Build indexes for base + head (CI / cron).
mallard index . --sha "$BASE_SHA" --out base.duckdb --rules rules.yml
mallard index . --sha "$HEAD_SHA" --out head.duckdb --rules rules.yml

# 2. Per changed file, list head symbols not in base (added or modified).
git diff --name-only "$BASE_SHA" "$HEAD_SHA" | while read -r f; do
  comm -23 \
    <(mallard query symbols-in-file "$f" --index head.duckdb | jq -r '.value[].id' | sort) \
    <(mallard query symbols-in-file "$f" --index base.duckdb | jq -r '.value[].id' | sort)
done

# 3. Per changed symbol, expand outbound calls (callees) and inbound calls (callers)
#    on the head index to bound the blast radius.
# 4. Attach `findings --symbol-id <id>` for each changed symbol — these are the
#    deterministic-hard signals from ADR-0009.
# 5. Hand the assembled (changed-symbol, neighbors, findings) tuples to the LLM
#    for synthesis. Every emitted comment must cite the symbol IDs / edge paths
#    / rule IDs it depends on.
```

## Notes

- Symbol IDs are stable for a given `(file path, qualified name, kind, signature)`. `comm` on sorted ID lists is a correct set-difference for added / removed; modified symbols get new IDs because signature is part of the hash.
- The base-index step (2) is optional for first-pass triage. Skipping it means treating every head symbol as "interesting"; useful only on tiny diffs.
- Per ADR-0009, deterministic findings *can* gate merges; LLM-synthesized comments cannot.
