# Examples

## Example `docs/` tree

```
docs/
  system.md

  specs/
    auth/
      identity.md
      permissions.md

    agents/
      tool-execution.md
      approvals.md
      audit-log.md

    data/
      invoice-matching.md
      transaction-labeling.md

    integrations/
      slack-release-notifications.md
      google-drive-import.md

    operations/
      deployment.md
      observability.md

  decisions/
    0001-use-better-auth.md
    0002-keep-agent-plans-ephemeral.md
```

## `system.md` — map, not territory

Good:

```md
## Agent execution

Agents can execute tools only through the tool execution layer.

See:
- `docs/specs/agents/tool-execution.md`
- `docs/specs/agents/approvals.md`
- `docs/specs/agents/audit-log.md`
```

Bad: copy-pasting full content of every related spec into `system.md`.

## Spec naming

Good (capability-shaped):

```
docs/specs/agents/tool-execution.md
docs/specs/auth/permissions.md
docs/specs/integrations/slack-release-notifications.md
docs/specs/data/invoice-matching.md
```

Bad (ticket-shaped or vague):

```
docs/specs/add-slack-release-notes.md
docs/specs/refactor-permissions.md
docs/specs/fix-agent-bug.md
docs/specs/active/add-tool-execution.md
docs/specs/auth.md
docs/specs/misc.md
```

## Prefer updating existing specs

Scenario: Slack release notes are added.

Create or update:

```
docs/specs/integrations/slack-release-notifications.md
```

Later the Slack message format changes — update the same file.

Do NOT create:

```
docs/specs/integrations/change-slack-message-format.md
```

The spec is the durable source of truth for the capability, across all future changes.

## Spec body anti-patterns

A spec describes durable behavior. It is not a changelog, a tutorial, or a code dump.

Avoid:

- **Changelog tone** — "Previously this returned X, now it returns Y." State the current contract; PR history covers the rest.
- **Implementation details** — class names, function names, file paths inside the spec body. Specs survive refactors; code references do not.
- **Long code listings** — paste only the minimum snippet needed to disambiguate a rule. Link to the source for the rest.
- **Step-by-step "how to add this" guides** — that's a tutorial. Specs describe what must hold, not how it was built.
- **Open questions and TODOs** — "should we also do X?" belongs in an issue or PR. Specs assert; they do not deliberate.
- **Ticket numbers and dates in the body** — couple the spec to the capability, not to the change that introduced it.
- **Duplication of another spec's rules** — link instead. Each rule lives in exactly one spec.

## Ephemeral scratch space

If agents need scratch space, use a single ignored folder:

```
.ai/
```

In `.gitignore`:

```gitignore
.ai/
```

Do not use `.agent/` (singular) — it visually collides with the `.agents/skills/` directory convention used by Claude skills.
