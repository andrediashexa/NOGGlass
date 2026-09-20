# NOGGlass

A multi-vendor, self-hosted looking glass for ISPs and network operators.

> The repository is named `looking-glass`; the product is **NOGGlass**
> ([ADR-0013](docs/adr/0013-product-name-and-branding.md)).

## TL;DR

NOGGlass lets your visitors run read-only diagnostics — `ping`, `traceroute`,
BGP and routing lookups — against your production routers from a web page, in
Portuguese, English or Spanish. BGP answers are not dumped as CLI text: they are
parsed into a normalised path model and drawn as an AS-PATH topology graph with
RPKI state, route attributes and decoded communities. It is free software
(Apache-2.0) and ships as a single Rust binary, or as one container.

> **Status: pre-alpha.** Not usable yet. The repository holds the project rules,
> the architecture decisions and a vendor parser crate. There is no SSH
> transport, no HTTP server and no interface yet. Follow the milestones.

## Why another looking glass

Most existing tools assume a single vendor, expose a shell-ish command box to
the Internet, or have been unmaintained for years — and all of them answer with
raw CLI text that an operator has to read line by line. NOGGlass targets the
network mix that regional ISPs actually run — MikroTik, Huawei, Datacom, Cisco,
Juniper, Nokia, Arista, FRR/BIRD — treats the public endpoint as what it is,
untrusted input landing next to a production router, and turns the answer into
something a NOC can read at a glance.

## Design principles

1. **The router is sacred.** A command is never built from raw user input. Every
   query is a typed request mapped to a vendor command with validated arguments.
2. **Read-only by contract.** Least-privilege router users, documented per
   vendor; anything outside the command catalogue is refused.
3. **Never invent data.** A field the router did not report is reported as
   unknown. A diagnostic tool that guesses is worse than one that is missing.
4. **Public means hostile.** Rate limiting, per-router concurrency caps, command
   timeouts and optional CAPTCHA are core, not add-ons.
5. **Self-hosting must be boring.** One binary or one container, one
   configuration file, published images, no build step on the operator's server.
6. **Safe defaults.** Anything that reaches the outside world starts disabled.

## Architecture

```mermaid
flowchart TB
    visitor([Visitor browser])
    proxy["Reverse proxy — optional<br/>TLS · ACME · edge rate limit"]

    subgraph binary [Single Rust binary]
        http[Axum HTTP + SSE<br/>listens on :8080]
        ui[Embedded UI<br/>pt / en / es · AS-PATH graph]
        valid[Typed target validation]
        limit[Rate limit + per-router concurrency]
        drivers[Vendor drivers]
        norm[Path model normaliser]
    end

    enrich["Enrichment — optional<br/>RPKI · AS names · RIPEstat"]
    routers[(Operator routers<br/>read-only SSH user)]

    visitor -->|HTTPS| proxy --> http
    visitor -.->|standalone| http
    http --> ui
    http --> valid --> limit --> drivers
    drivers -->|SSH| routers
    drivers --> norm --> http
    norm -.-> enrich
```

## Stack

| Layer | Choice | Decision |
|---|---|---|
| Language | Rust | [ADR-0005](docs/adr/0005-backend-implementation-language.md) |
| Server and interface | Axum, with the UI embedded in the binary | [ADR-0007](docs/adr/0007-unified-rust-architecture-and-drivers.md) |
| Languages | pt-BR, en, es by URL prefix | [ADR-0004](docs/adr/0004-web-interface-internationalisation.md) |
| Router access | Direct SSH, read-only user, per-vendor drivers | [ADR-0007](docs/adr/0007-unified-rust-architecture-and-drivers.md) |
| BGP results | Normalised path model, never raw text only | [ADR-0006](docs/adr/0006-structured-bgp-model-and-data-sources.md) |
| Deployment | Binary on a high port; reverse proxy optional | [ADR-0012](docs/adr/0012-standalone-binary-exposure-and-packaging.md) |
| Releases | Conventional Commits, SemVer, release-please | [ADR-0003](docs/adr/0003-conventional-commits-and-automated-releases.md) |

## Documentation

All documentation lives in [`docs/`](docs/), in English, following the
[documentation standard](docs/process/documentation-standard.md).

| Document | Purpose |
|---|---|
| [Architecture overview](docs/architecture/overview.md) | Components, request flow and trust boundaries |
| [Architecture decisions](docs/adr/) | Why the project looks like this |
| [Versioning and releases](docs/process/versioning-and-releases.md) | How a merge becomes a release |
| [Contributing](CONTRIBUTING.md) | Issue, branch, commit and review rules |
| [Security policy](SECURITY.md) | How to report a vulnerability |

## License

[Apache License 2.0](LICENSE).
