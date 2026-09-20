# Project rules for Claude Code

## TL;DR

This is **NOGGlass**, a multi-vendor, self-hosted looking glass: one Rust binary
serving the API and an embedded UI in pt/en/es, read-only SSH to routers, BGP
answers as a normalised path model drawn as an AS-PATH graph. Everything written
here is in English. One issue per feature, one branch per issue, squashed pull
request, automatic release on merge. Never put user input on a router command
line, and never invent a value a router did not report.

## Working agreement

- Speak Portuguese with the maintainers in chat; write English in the
  repository — code, comments, commits, issues, pull requests, documentation.
- `main` is protected. Never commit to it directly: create
  `<type>/<issue>-<slug>`, open a pull request with a Conventional Commits title
  and `Closes #<issue>` in the body.
- Every feature starts as a GitHub issue. If a task arrives without one, create
  the issue first.
- Do not edit `version.txt` or `CHANGELOG.md`; `release-please` owns them.
- Every merge is tagged `vX.Y.Z` and released; patch releases are published as
  pre-releases, minor and major as full releases (ADR-0014).
- Run `scripts/check-docs.sh` and `scripts/check-i18n.sh` before pushing, plus
  `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test` when Rust changed.
- This machine has no C toolchain and no sudo, but `russh` pulls in `ring`,
  which needs one. zig provides it: `~/.local/bin/zig-cc` and `zig-ar` wrap
  `zig cc` and `zig ar`, rewriting only the target triple (rewriting every
  occurrence mangles the sysroot paths). Build with:
  `CC=zig-cc AR=zig-ar cargo test --config 'target.x86_64-unknown-linux-gnu.linker="zig-cc"'`.
  CI has a normal toolchain and needs none of this.

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
   vendor means refusing the query, never falling back to a guess. A parser that
   cannot read a field MUST report that it could not: unimplemented parsers
   return an error, absent attributes are null, and `Ok(empty)` MUST NOT be
   used to mean failure.
4. **Safe defaults.** Every feature flag that reaches the outside world defaults
   to disabled (`*_ENABLED=false`).
5. **Pinned dependencies.** `Cargo.lock` committed, `rust-toolchain.toml`
   pinned, fixed tags for third-party images.
6. **Memory limits are measured.** Every Compose service declares `mem_limit`
   with a comment stating the measured usage.
7. **No published ports except the proxy.** Internal services talk over the
   Compose network by name.
8. **User-facing strings live in translation files** for pt-BR, en and es —
   never hard-coded in components.

## Stack

| Layer | Choice |
|---|---|
| Language | Rust (ADR-0005), workspace under `crates/` |
| Server | Axum + Tokio, SSE for streamed output, listens on :8080 (ADR-0012) |
| Interface | Embedded in the binary, i18n by URL prefix (`/pt`, `/en`, `/es`) (ADR-0007) |
| Router access | Direct SSH (`russh`), read-only user, per-vendor drivers |
| BGP results | Normalised path model with raw output preserved (ADR-0006) |
| Packaging | Single binary; multi-stage image published to GHCR (ADR-0012) |
| CI | GitHub Actions: cargo fmt/clippy/test, docs lint, i18n parity, secret scan |
| Release | release-please, Conventional Commits, SemVer |
