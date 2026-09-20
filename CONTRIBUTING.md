# Contributing

## TL;DR

One issue per feature, one branch per issue, one squashed pull request per
branch, one automatic release per merge. Titles follow Conventional Commits,
everything written in the repository is in English, and every document follows
the [documentation standard](docs/process/documentation-standard.md). CI blocks
what does not comply.

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD",
"SHOULD NOT", "RECOMMENDED", "MAY" and "OPTIONAL" in this document are to be
interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

## 1. Language

- Code, comments, identifiers, issues, pull requests, commit messages and
  documentation MUST be written in English.
- Only the web interface is multilingual (Portuguese, English, Spanish), through
  translation files. Hard-coded user-facing strings MUST NOT be committed.

## 2. Issues

- Every feature, bug fix and behaviour change MUST have an issue before the
  pull request is opened.
- Trivial `chore` and `docs` changes (typos, formatting, dependency bumps) MAY
  skip the issue.
- Issues MUST be created from one of the templates and MUST carry a `type:` and
  an `area:` label.
- An issue SHOULD state the acceptance criteria as a checklist. Work starts when
  it is clear what "done" means.

## 3. Branches

- Work MUST NOT happen on `main`. Every change starts from a fresh branch off
  `main`, one branch per issue, and reaches `main` only through a pull request.
- History on `main` is never rewritten: no force pushes, no rebases of merged
  work.
- Branch names MUST follow `<type>/<issue-number>-<short-slug>`:

  ```text
  feat/12-ssh-executor
  fix/31-mikrotik-timeout
  docs/44-vendor-matrix
  ```

- Allowed types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `perf`,
  `ci`, `build`, `style`, `revert`.

## 4. Commits and pull request titles

- The pull request title MUST follow
  [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):
  `<type>(<scope>): <description>`.
- The title is what lands in `main` (squash merge) and is what drives the
  version bump, so it MUST describe the change from a user's point of view.
- A breaking change MUST be marked with `!` (`feat(api)!: ...`) or a
  `BREAKING CHANGE:` footer.
- Scopes in use: `api`, `web`, `executor`, `vendor`, `infra`, `docs`, `ci`,
  `i18n`, `deps`.

```text
feat(executor): add MikroTik RouterOS driver
fix(api): reject IPv6 prefixes longer than /64 in ping
docs(adr): record the direct-SSH decision
```

## 5. Pull requests

- Every pull request MUST reference its issue in the body with a closing
  keyword: `Closes #12`.
- Merges MUST be squash merges. Merge commits and rebase merges are disabled.
- All required checks MUST pass before merging.
- Approval from the other maintainer is REQUIRED for changes to the backend,
  the SSH executor, vendor drivers or anything under `.github/`. See
  [CODEOWNERS](.github/CODEOWNERS). Other areas MAY be self-merged once CI is
  green — with two maintainers, blocking every change on a review would stall
  the project more than it protects it.
- Pull requests SHOULD stay under roughly 400 changed lines. Split larger work.

### What enforces what

Be honest about the guardrails, so nobody relies on one that is not there.

| Rule | Enforced by |
|---|---|
| Conventional Commits title | CI (`pr-checks`), blocking |
| Branch naming | CI (`pr-checks`), blocking |
| Issue reference in the body | CI (`pr-checks`), blocking |
| Documentation standard | CI (`docs-lint`), blocking |
| Translation parity | CI (`docs-lint`), blocking |
| No committed secret | CI (`secret-scan`), blocking |
| Squash-only merges | Repository setting |
| No direct push to `main` | **Convention only** |

GitHub requires a paid plan to protect a branch in a private repository, and
this repository is private for now. Branch protection — required checks, no
direct pushes, linear history — SHALL be enabled as soon as the repository
becomes public or the account gains that capability. Until then, pushing to
`main` is technically possible and MUST NOT be done.

## 6. Releases

Every merge into `main` produces a tag `vMAJOR.MINOR.PATCH` and a GitHub
release automatically — documentation and tooling changes included. A patch
release is published as a **pre-release**; minor and major releases are full
releases (ADR-0014). The rules are described in
[versioning and releases](docs/process/versioning-and-releases.md).
Contributors do not edit `version.txt` or `CHANGELOG.md` by hand.

## 7. Documentation

- Any change that alters behaviour, configuration or architecture MUST update
  the documentation in the same pull request.
- Documents MUST follow the [documentation standard](docs/process/documentation-standard.md):
  English, TL;DR, 5W2H, RFC 2119 keywords and Mermaid diagrams.
- An architecture decision MUST be recorded as an ADR in [`docs/adr/`](docs/adr/).

## 8. Security rules for contributions

- User input MUST NOT reach a router command line. Requests are typed, validated
  and mapped to templates from the command catalogue.
- New router-facing commands MUST be read-only and MUST be added to the
  catalogue with a timeout and an output size cap.
- Anything that acts outside the process (sending mail, calling an external API,
  writing to a router) MUST ship behind a kill switch that defaults to disabled.
- Secrets MUST NOT be committed. `gitleaks` runs on every pull request.
- Suspected vulnerabilities MUST follow [SECURITY.md](SECURITY.md) instead of a
  public issue.
