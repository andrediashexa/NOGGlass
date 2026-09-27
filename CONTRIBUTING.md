# Contributing

## TL;DR

One issue per feature, one branch per issue, one squashed pull request per branch, one automatic release per merge. Titles follow Conventional Commits, code and documentation are in English, and every document follows the [documentation standard](docs/process/documentation-standard.md). AI-assisted contributions are welcome provided the author assumes full responsibility and validates all code. CI blocks what does not comply.

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY" and "OPTIONAL" in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

---

## 1. Code of Conduct and Collaboration

All contributors and participants are expected to uphold our [Code of Conduct](CODE_OF_CONDUCT.md). Treat others with respect, focus on technical substance, welcome constructive criticism, and assume good faith.

---

## 2. Language and Communication

- Code, comments, identifiers, issues, pull requests, commit messages, and documentation MUST be written in English.
- Only the web interface is multilingual (Portuguese, English, Spanish), managed via translation catalogues (`crates/nogglass-server/ui/messages/`). Hard-coded user-facing strings MUST NOT be committed.

---

## 3. Issues and Bug Reporting

- Every feature, bug fix, and behaviour change MUST have an issue before the pull request is opened.
- Trivial `chore` and `docs` changes (typos, formatting, dependency bumps) MAY skip the issue.
- Issues MUST be created using the repository issue templates and carry appropriate `type:` and `area:` labels.

### When Reporting Bugs
Please provide enough operational context to reproduce the problem:
- Operating system and deployment method (Docker Compose or native Systemd binary).
- Rust compiler version (`rustc --version`).
- Target router hardware, vendor driver, and operating system firmware version (e.g. Huawei VRP V800R012, JunOS 21.4R3, RouterOS v7.14).
- Minimal reproduction steps, expected behaviour, and actual CLI/API output.
- Relevant log lines (with internal secrets or IP addresses scrubbed or substituted with RFC 5737 documentation ranges).

---

## 4. Branches and Workflow

- Work MUST NOT happen on `main`. Every change starts from a fresh branch off `main`, one branch per issue, and reaches `main` only through a pull request.
- History on `main` is never rewritten: no force pushes, no rebases of merged work.
- Branch names MUST follow `<type>/<issue-number>-<short-slug>`:

  ```text
  feat/12-ssh-executor
  fix/31-mikrotik-timeout
  docs/44-vendor-matrix
  ```

- Allowed types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `perf`, `ci`, `build`, `style`, `revert`.

---

## 5. Commits and Pull Request Titles

- The pull request title MUST follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `<type>(<scope>): <description>`.
- The title is what lands in `main` (squash merge) and drives automated SemVer version bumps.
- Breaking changes MUST be marked with `!` (`feat(api)!: ...`) or a `BREAKING CHANGE:` footer.
- Scopes in use: `api`, `web`, `executor`, `vendor`, `infra`, `docs`, `ci`, `i18n`, `deps`.

```text
feat(executor): add MikroTik RouterOS driver
fix(api): reject IPv6 prefixes longer than /64 in ping
docs(adr): record the direct-SSH decision
```

---

## 6. Pull Requests and Review

- Every pull request MUST reference its issue in the body with a closing keyword: `Closes #12`.
- Merges MUST be squash merges. Merge commits and rebase merges are disabled.
- All required CI checks MUST pass before merging.
- Approval from the other maintainer is REQUIRED for changes to the backend, the SSH executor, vendor drivers, or anything under `.github/`. See [CODEOWNERS](.github/CODEOWNERS).
- Pull requests SHOULD stay under roughly 400 changed lines. Split larger features into incremental steps.

### Enforcement Matrix

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

---

## 7. Code Quality and Engineering Standards

Submissions must reflect production-grade engineering:
- **Readability and Simplicity:** Code must be clear, idiomatic Rust with clear variable naming and modular design.
- **KISS & YAGNI:** Avoid premature abstractions or complex generic hierarchies when a simple concrete implementation suffices.
- **Performance with Justification:** Minimize memory allocations, avoid unnecessary `.clone()` calls in hot paths, and document algorithmic complexity where non-obvious.
- **RFC Compliance in Tests:** All automated tests MUST strictly use documentation IP prefixes (RFC 5737 / RFC 3849) and private ASNs (RFC 6996). Real production IPs or public ASNs MUST NOT appear in test suites.

---

## 8. AI-Assisted Contributions

AI-assisted contributions (using LLMs, Copilot, Claude, or other coding assistants) are welcome, subject to the engineering principles defined in [DEVELOPMENT_PHILOSOPHY.md](DEVELOPMENT_PHILOSOPHY.md):

- **Human Accountability:** Contributors remain 100% responsible for all submitted code, documentation, and logic.
- **Mandatory Human Verification:** Every line of AI-generated code MUST be reviewed, understood, and tested before submission. Unreviewed code dumps or hallucinations will be rejected.
- **Deep Understanding:** Contributors must be able to explain architectural decisions, edge cases, and algorithmic trade-offs during code review.
- **Zero Hallucinated Dependencies:** Do not introduce unvetted third-party crates or unnecessary runtime dependencies.

---

## 9. Security Rules for Contributions

- User input MUST NOT reach a router command line unvalidated. All requests are strictly typed (`std::net::IpAddr`, `ipnet::IpNet`), validated, and mapped to templates from the command catalogue.
- New router-facing commands MUST be strictly read-only and added to the catalogue with timeouts and buffer size caps.
- Anything acting outside the process (external API calls, network probes) MUST ship behind a kill switch that defaults to disabled.
- Secrets MUST NOT be committed.
- Suspected security vulnerabilities MUST follow [SECURITY.md](SECURITY.md) instead of public issues.

---

## 10. Releases and Documentation

- Every merge into `main` produces an automated tag `vMAJOR.MINOR.PATCH` and a GitHub release via Google Release-Please. Contributors do not edit `version.txt` or `CHANGELOG.md` manually.
- Any change altering behavior, configuration, or architecture MUST update the documentation in the same pull request.
- Documents MUST follow the [documentation standard](docs/process/documentation-standard.md): English, TL;DR, 5W2H, RFC 2119 keywords, and Mermaid diagrams.
- Architectural decisions MUST be formally recorded as an ADR in [`docs/adr/`](docs/adr/).
