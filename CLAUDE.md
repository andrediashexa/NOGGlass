# Project rules for Claude Code

## TL;DR

This is a multi-vendor, self-hosted Looking Glass: FastAPI backend, Next.js
frontend in pt/en/es, read-only SSH to routers, shipped as Docker Compose.
Everything written here is in English. One issue per feature, one branch per
issue, squashed pull request, automatic release on merge. Never put user input
on a router command line.

## Working agreement

- Speak Portuguese with the maintainers in chat; write English in the
  repository — code, comments, commits, issues, pull requests, documentation.
- `main` is protected. Never commit to it directly: create
  `<type>/<issue>-<slug>`, open a pull request with a Conventional Commits title
  and `Closes #<issue>` in the body.
- Every feature starts as a GitHub issue. If a task arrives without one, create
  the issue first.
- Do not edit `version.txt` or `CHANGELOG.md`; `release-please` owns them.
- Run `scripts/check-docs.sh` and `scripts/check-i18n.sh` before pushing.

## Documentation rules

Every document under `docs/` MUST follow
`docs/process/documentation-standard.md`: English, a `## TL;DR` section, a
`## 5W2H` section, RFC 2119 keywords in uppercase and at least one Mermaid
diagram when describing a flow or a structure. The docs linter enforces this in
CI, so write documents from `docs/templates/document-template.md`.

Architecture decisions go to `docs/adr/` as numbered ADRs.

## Non-negotiable engineering rules

1. **No shell interpolation towards routers.** A query is a typed request
   (`ping`, `traceroute`, `bgp_route`, ...) resolved against a per-vendor
   command template with validated, escaped arguments. Never build a command
   string from what a visitor typed.
2. **Read-only only.** Commands that change router state are out of scope,
   including in drivers written "just for testing".
3. **Fail closed.** Missing configuration, missing credentials or an unknown
   vendor means refusing the query, never falling back to a guess.
4. **Safe defaults.** Every feature flag that reaches the outside world defaults
   to disabled (`*_ENABLED=false`).
5. **Pinned dependencies.** Exact versions in `requirements.txt`, lockfile for
   the frontend, fixed tags for third-party images.
6. **Memory limits are measured.** Every Compose service declares `mem_limit`
   with a comment stating the measured usage.
7. **No published ports except the proxy.** Internal services talk over the
   Compose network by name.
8. **User-facing strings live in translation files** for pt-BR, en and es —
   never hard-coded in components.

## Stack

| Layer | Choice |
|---|---|
| Frontend | Next.js + TypeScript, i18n by URL prefix (`/pt`, `/en`, `/es`) |
| Backend | Python 3.12 + FastAPI, SSE for streaming output |
| Router access | Direct SSH from the backend (scrapli/netmiko), read-only user |
| Proxy | Traefik v3 |
| Packaging | Docker Compose, images published to GHCR |
| CI | GitHub Actions: docs lint, i18n parity, secret scan, tests |
| Release | release-please, Conventional Commits, SemVer |
