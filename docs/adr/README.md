# Architecture Decision Records

## TL;DR

Each ADR records one decision: the context, what was decided, and what it costs.
ADRs are immutable once merged — a change of mind becomes a new ADR that
supersedes the old one. Numbering is sequential, starting at `0001`.

## 5W2H

| Question | Answer |
|---|---|
| **What** | The decision log of the project. |
| **Why** | So a decision is discussed once and its reasoning survives the conversation that produced it. |
| **Who** | Maintainers; contributors MAY propose an ADR in a pull request. |
| **Where** | `docs/adr/NNNN-kebab-case-title.md`. |
| **When** | Written before or together with the implementing pull request. |
| **How** | Copy `docs/templates/document-template.md` and keep Status, Context, Decision, Consequences. |
| **How much** | No cost; typically half an hour of writing. |

## Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Proposed
    Proposed --> Accepted: merged
    Proposed --> Rejected: closed
    Accepted --> Superseded: a newer ADR replaces it
    Superseded --> [*]
    Rejected --> [*]
```

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-fastapi-backend-and-direct-ssh.md) | FastAPI backend with direct SSH to routers | Accepted |
| [0003](0003-conventional-commits-and-automated-releases.md) | Conventional Commits and automated releases | Accepted |
| [0004](0004-web-interface-internationalisation.md) | Web interface internationalisation by URL prefix | Accepted |
