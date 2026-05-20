---
name: spec-driven-docs
description: Minimal spec-driven documentation model for projects. Use when creating, organizing, or maintaining a repo's `docs/` directory; deciding whether a change needs a doc update; writing or updating capability specs, ADRs, or `system.md`; reviewing PRs for documentation hygiene; or migrating an existing `docs/` tree into the spec/decision/system layout.
---

# Spec-Driven Documentation

Persist only documentation with durable value: what the system must do, what contracts must hold, why major decisions were made, how the system fits together.

Everything else (task lists, plans, scratchpads, PR history, change archives) belongs in issues, PRs, commits, or ignored local folders.

**Durable behavior** = a contract a future change must respect: authorization rules, data contracts, API behavior, external integrations, money/billing rules, user-visible workflow rules, production operational behavior.

## Three persistent doc types

```
docs/
  system.md                              # system map: modules, flows, links
  specs/<domain>/<capability>.md         # durable behavior per capability
  decisions/<NNNN>-<slug>.md             # ADRs for major choices
```

- **`system.md`** — map, not territory. List modules, data flows, integrations, deployment shape. Link to specs/decisions; do not duplicate them.
- **specs** — name by capability, not ticket. State purpose, behavior, rules, I/O, edge cases, observability.
- **decisions** — ADRs. Capture context, decision, alternatives, consequences. Numbered.

## Forbidden in persistent docs

```
tasks.md  plan.md  todo.md  scratch.md  notes.md
active/   done/    archive/  changes/
```

Use `.ai/` (gitignored) for ephemeral agent scratch. Do not use `.agent/` — it visually collides with the `.agents/skills/` convention.

## When to create or update

- **Update an existing spec** when durable behavior changes. Default action.
- **Create a new spec** only when a new durable capability appears.
- **Create an ADR** only for major architectural decisions worth not re-litigating.
- **No doc change** for typo fixes, UI polish, one-off scripts, no-behavior refactors, experiments, task-level implementation.

See [references/triggers.md](references/triggers.md) for the full durable-vs-not trigger list.

## Naming

Lowercase kebab-case. Capability-shaped, not ticket-shaped.

- Good: `tool-execution.md`, `slack-release-notifications.md`, `invoice-transaction-matching.md`
- Bad: `add-auth.md`, `fix-agent-permissions.md`, `auth.md`, `misc.md`

ADR files: zero-pad to 4 digits, next sequential number, never reuse. Example: `0007-keep-agent-plans-ephemeral.md`.

See [references/examples.md](references/examples.md) for fuller good/bad lists and spec-body anti-patterns.

## Workflow for a change

1. Understand the request.
2. Check whether an existing spec applies.
3. If durable behavior changes, update that spec.
4. Keep the implementation plan ephemeral (chat/PR description).
5. Implement and test.
6. Update `system.md` only if the system map changed.
7. Add an ADR only if a major decision was made.

For trivial changes: implement, test, do not touch docs.

## Bootstrapping a new repo

When `docs/` does not yet exist:

1. Create `docs/system.md` with sections: **Overview**, **Modules**, **Data flows**, **Integrations**, **Deployment**, **Related specs/decisions**. Stub each — fill as the system grows.
2. Create empty `docs/specs/` and `docs/decisions/` directories (add `.gitkeep` if needed).
3. Add `.ai/` to `.gitignore`.
4. Optionally drop `assets/pr-checklist.md` into the PR template.

Do not pre-create domain folders. Add `docs/specs/<domain>/` when the first capability in that domain appears.

## Templates

Ready-to-copy templates in `assets/`. Read and copy when creating a new file.

- `assets/spec-template.md` — standard capability spec
- `assets/data-spec-template.md` — data-heavy variant (sources, quality checks)
- `assets/adr-template.md` — architecture decision record
- `assets/pr-checklist.md` — drop into PR template or copy into PR description

## Linking

Keep `## Related` links bidirectional. When spec A references spec B, add A to B's `## Related` section too. Same for spec ↔ ADR.

## Migrating an existing `docs/` tree

See [references/migration.md](references/migration.md) for keep / move / delete / merge rules and worked examples.

## Agent rules

1. Read `docs/system.md` before broad architectural changes.
2. Check `docs/specs/` before changing durable behavior.
3. Update an existing spec rather than creating a new one when possible.
4. Never add persistent plans, task lists, scratchpads, or active/done/archive folders.
5. Prefer fewer, better docs over many stale docs.
