# Security policy

## Supported versions

Mallard is pre-1.0. Only the latest release on `main` receives security fixes.

| Version | Supported |
|---------|-----------|
| `main`  | ✓         |
| < `main`| ✗         |

## Reporting a vulnerability

If you discover a security vulnerability in mallard:

1. **Do not open a public issue.**
2. Email Richárd Gyikó (repository owner — contact through GitHub profile) **or** use GitHub's [private vulnerability reporting](https://github.com/richard-gyiko/mallard/security/advisories/new) on this repository.
3. Include:
   - A description of the issue
   - Steps to reproduce
   - Affected versions
   - Suggested mitigation if you have one

## Response targets

- **Acknowledgment:** within 7 days
- **Initial assessment:** within 14 days
- **Fix or workaround:** scoped per severity, prioritized for critical issues

## Scope

Mallard runs entirely locally on the user's machine or CI runner. It makes no network calls. Per ADR-0013, mallard ships zero LLM integration — your code never leaves the runner.

**In scope** for security reports:

- Crashes or panics on malformed input that could be triggered remotely (e.g., a malicious repo)
- Path traversal in `mallard index` against a controlled repo
- DuckDB index corruption that could be weaponized
- Dependency vulnerabilities surfaced via `cargo audit`

**Out of scope:**

- Issues requiring a malicious local actor with write access to your `.mallard/` index
- Issues in dependencies that the dependency's maintainers have already triaged
- Performance regressions (file a regular issue)

## Disclosure

Once a fix lands, we coordinate disclosure timing with the reporter. Default: 90 days from acknowledgment, sooner for low-impact issues, longer only if the reporter requests it.
