# Versioning and releases

## TL;DR

The project follows Semantic Versioning. Pull request titles follow Conventional
Commits, `release-please` reads them after every squash merge into `main`,
bumps `version.txt`, writes `CHANGELOG.md`, tags `vX.Y.Z` and publishes a GitHub
Release with container images. Nobody edits version numbers by hand, and the
running version is always visible in the web interface footer and at
`/api/version`.

## 5W2H

| Question | Answer |
|---|---|
| **What** | The automated path from a merged pull request to a tagged, released, visible version. |
| **Why** | Operators self-host this software and MUST be able to tell which version they run, what changed and whether an upgrade breaks them. |
| **Who** | `release-please` running in GitHub Actions; maintainers only approve its release pull request. |
| **Where** | `.github/workflows/release.yml`, `release-please-config.json`, `version.txt`, `CHANGELOG.md`. |
| **When** | On every push to `main`, which only happens through squash merges. |
| **How** | Conventional Commits determine the bump; see the flow below. |
| **How much** | No cost beyond GitHub Actions minutes, which are free for public repositories. |

## Version scheme

The project uses [Semantic Versioning 2.0.0](https://semver.org/):
`MAJOR.MINOR.PATCH`.

| Pull request title | Bump while `0.x` | Bump after `1.0.0` |
|---|---|---|
| `fix: ...` | patch | patch |
| `feat: ...` | minor | minor |
| `feat!: ...` or `BREAKING CHANGE:` | minor | major |
| `docs:`, `chore:`, `ci:`, `test:`, `refactor:`, `style:` | none | none |

While the version is below `1.0.0` the public interfaces MAY change between
minor versions. `1.0.0` SHALL be released when the Looking Glass runs
end-to-end: the supported vendor drivers, the public query flow, the three
languages and the deployment documentation.

A change that only touches documentation or CI does NOT produce a release. The
rule "one release per merge" applies to merges that change the product; a
documentation-only merge produces no artefact to release.

## Release flow

```mermaid
sequenceDiagram
    participant Dev as Maintainer
    participant PR as Pull request
    participant Main as main
    participant RP as release-please
    participant GH as GitHub Release
    participant GHCR as GHCR images

    Dev->>PR: Conventional Commits title + Closes #issue
    PR->>Main: squash merge
    Main->>RP: push event
    RP->>RP: read commits since last tag
    RP->>Main: open or update "chore(main): release X.Y.Z" PR
    Dev->>RP: merge the release PR
    RP->>GH: create tag vX.Y.Z + release notes
    GH->>GHCR: build and push images X.Y.Z, X.Y, latest
```

## Where the version becomes visible

| Surface | Content | Source |
|---|---|---|
| Web footer | `vX.Y.Z`, linking to the GitHub release | build-time environment variable |
| About page | version, short commit SHA, build date, changelog link | `/api/version` |
| `GET /api/version` | JSON with the same fields | `version.txt` and build arguments |
| Container image tags | `X.Y.Z`, `X.Y`, `latest` | release workflow |

The frontend reads the version at build time, so the release workflow MUST pass
it as a build argument. A container that cannot determine its version MUST
display `dev` rather than a wrong number.

## Rules

- Contributors MUST NOT edit `version.txt` or `CHANGELOG.md` manually.
- The release pull request SHOULD be merged as soon as CI is green; letting
  releases pile up defeats the purpose.
- A release MUST NOT be deleted or re-tagged. A mistake is fixed by releasing
  the next patch version.
- Tags MUST be `vX.Y.Z`, matching the GitHub Release name.
