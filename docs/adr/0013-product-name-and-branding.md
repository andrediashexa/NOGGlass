# ADR-0013 — Product name: NOGGlass

- **Status:** Accepted
- **Date:** 2026-09-20
- **Deciders:** André Dias, Marcelo Gondim

## TL;DR

The product is called **NOGGlass**. The name ships in the binary, the container
image, the user interface, the documentation and the eventual domain. The Git
repository keeps the name `looking-glass` for now, because renaming it breaks
existing clones and links for no functional gain; the repository name is not the
product name.

## 5W2H

| Question | Answer |
|---|---|
| **What** | The name the product carries everywhere a user can see it. |
| **Why** | "Looking glass" is a category, not a name: dozens of projects use it, so it cannot be searched for, trademarked or recognised. |
| **Who** | Both maintainers; affects every contributor writing user-facing strings. |
| **Where** | Binary name, image name, page title, interface header, documentation, GHCR path. |
| **When** | From the first published artefact of `0.1.0`. |
| **How** | `nogglass` as the machine-readable identifier, `NOGGlass` as the display form. |
| **How much** | No cost now; renaming after the first release would cost a deprecation cycle. |

## Context

The prototype and the interface design already carry the NOGGlass identity,
including the logo in `docs/assets/looking_glass_preview.jpg`. "NOG" refers to
Network Operators Group, which places the product in its audience immediately.
The repository was created earlier under the generic name `looking-glass`, from
before the product had an identity.

Mixing the two names across binary, image, docs and interface is how projects
end up with three names for one thing.

## Decision

1. The product name SHALL be **NOGGlass**, displayed exactly like that: capital
   N, O, G, G, lowercase "lass".
2. The machine-readable identifier SHALL be `nogglass`, in lowercase, used for:
   the binary, the container image (`ghcr.io/<owner>/nogglass`), the
   configuration directory (`/etc/nogglass`), and environment variables
   (`NOGGLASS_*`).
3. Documentation and user interface strings SHALL use "NOGGlass" and MAY
   describe it as "a multi-vendor looking glass" — the category stays lowercase
   and generic.
4. The repository SHALL keep the name `looking-glass` until there is a reason
   to rename it. If it is renamed, GitHub redirects old links, but local clones
   need their remote updated, so the change MUST be announced.
5. The name SHALL NOT be changed after `1.0.0` without a deprecation path for
   image names and configuration paths.

```mermaid
flowchart LR
    name[NOGGlass<br/>display name] --> ui[Interface header and page title]
    name --> docs[Documentation]
    id[nogglass<br/>identifier] --> bin[Binary]
    id --> img["Container image<br/>ghcr.io/owner/nogglass"]
    id --> cfg[/etc/nogglass]
    id --> env[NOGGLASS_* variables]
    repo[Repository: looking-glass] -.->|unchanged for now| name
```

## Consequences

- Environment variables and paths proposed in earlier ADRs using
  `LOOKING_GLASS_*` or `/etc/looking-glass` MUST be renamed before the first
  release, while nothing depends on them.
- A trademark search was NOT performed; if the name turns out to be taken in a
  relevant jurisdiction, the cost of changing grows sharply after `1.0.0`.
- The repository name and the product name differ, which MUST be stated in the
  README so nobody assumes two projects.
