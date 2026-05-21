# Migrating an existing `docs/` tree

## Keep

Move durable behavior into:

```
docs/specs/<domain>/<capability>.md
```

Move architectural choices into:

```
docs/decisions/<NNNN>-<slug>.md
```

Move high-level system overview into:

```
docs/system.md
```

## Delete or move out of persistent docs

Remove from the repo (or move to PRs/issues):

```
old task lists
old implementation plans
agent scratchpads
outdated proposals
duplicated notes
archived change folders
```

## Merge duplicates

If multiple docs describe the same capability, merge into one spec.

Prefer:

```
docs/specs/agents/tool-execution.md
```

over:

```
docs/specs/tool-calling.md
docs/specs/agent-tools.md
docs/specs/tool-auth.md
docs/specs/execute-tool-feature.md
```

unless those genuinely describe separate durable capabilities.

## Final principle

The repo should not preserve every step of how work happened. It should preserve:

```
what the system does
what contracts must hold
why major decisions were made
how the system fits together
```

Everything else lives in issues, PRs, conversations, commits, or ephemeral agent notes.
