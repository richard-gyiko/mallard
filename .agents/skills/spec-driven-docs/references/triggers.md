# Durable-vs-not triggers

## Update or create a spec when the change affects

- authorization or permissions
- agent tool execution
- data contracts (schemas, field semantics, IDs)
- API behavior (request/response shape, status codes, idempotency, ordering)
- external integrations (webhooks, third-party APIs, file formats)
- billing, invoices, reports, money rules
- production operational behavior (retries, timeouts, rate limits, alerting)
- user-visible workflow rules (approval flows, state machines)
- repeated ambiguity that keeps resurfacing in reviews or incidents
- behavior future agents are likely to misunderstand from the code alone

## Do NOT create a spec for

- typo fixes
- UI polish, copy tweaks, styling
- one-off scripts and ad-hoc data backfills
- refactors with no behavior change
- temporary experiments and feature flags pre-launch
- implementation tasks (how to build, not what must hold)
- bug fixes that restore documented behavior — fix the code, do not duplicate the spec

## Edge cases

- **Bug fix that reveals undocumented behavior** — write the spec now. The bug proved the behavior was load-bearing.
- **Feature flag rollout** — spec the *behind-the-flag* behavior only when it ships to users; before that, keep notes in the PR.
- **Deprecation** — update the spec to mark deprecated rules and link to the replacement spec. Do not delete until the code is gone.
