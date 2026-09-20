# ADR-0007 — End-to-end Rust architecture with embedded web UI and native vendor drivers

- **Status:** Accepted
- **Date:** 2026-09-17 (accepted 2026-09-20)
- **Deciders:** André Dias, Marcelo Gondim

## TL;DR

Proposes an all-in-one, end-to-end Rust architecture that implements both the backend API
and the frontend UI in Rust, eliminating external web servers (Nginx, Apache, Traefik),
Node.js, and Python. Router communication replaces Netmiko with native asynchronous SSH (`russh`)
tailored strictly to read-only diagnostics, embedding UI assets directly into a single static
binary with built-in TLS, rate-limiting and under 35 MB of RAM footprint.

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT",
"RECOMMENDED", "MAY" and "OPTIONAL" in this document are to be interpreted as described in
[RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

## 5W2H

| Question | Answer |
|---|---|
| **What** | Complete Rust stack (Axum HTTP/SSE server + embedded web UI + native SSH drivers). |
| **Why** | Eliminate intermediate web servers (Nginx/Apache), remove Node and Python runtimes, and maximize security. |
| **Who** | Maintainers, contributors, and ISP network operators. |
| **Where** | Implemented under `crates/looking-glass-core/` and the main service binary. |
| **When** | Proposed during the pre-alpha architecture decision phase. |
| **How** | Pure Rust using Axum, `rustls` (built-in HTTPS/TLS), embedded UI templates, and `russh` async drivers. |
| **How much** | Single static executable (~15-20 MB), RAM under 35 MB, zero external reverse proxy required. |

## Context

Previous documents considered a dual-runtime or multi-container architecture:
- ADR-0002 adopted Python (FastAPI) and Next.js, relying on Traefik as reverse proxy.
- ADR-0005 debated Python vs Go vs Rust for the backend, while keeping Next.js for frontend delivery.

In real-world ISP operations, managing multiple containers (Node.js for Next.js, Python for FastAPI,
plus Nginx, Apache2 or Traefik for reverse proxy and TLS termination) creates operational friction:
1. **Container & Web Server Sprawl:** Running separate containers for frontend, backend, and reverse proxy
   consumes 500 MB to 1.5 GB of RAM just for orchestration and idle runtimes.
2. **Reverse Proxy Redundancy:** Modern Rust HTTP engines (such as **Axum** on top of **Hyper** and **`rustls`**)
   provide enterprise-grade, memory-safe HTTP/1.1, HTTP/2, and native TLS termination. Requiring Nginx or
   Apache in front of an Axum binary is unnecessary overhead for a dedicated appliance.
3. **The Netmiko Fallacy:** Netmiko's 10-year codebase is massive because it handles stateful write operations
   (`config term`, rollback, syntax checks, confirmation prompts). A Looking Glass executes only
   **read-only commands** (`ping`, `traceroute`, `show route`, `show bgp`). We only need the vendor-specific
   pagination commands and prompt regular expressions.

## Decision

1. **100% Rust Backend & Frontend Delivery:**
   - The entire Looking Glass application SHALL be compiled into a **single static binary** written in Rust.
   - The HTTP and Server-Sent Events (SSE) server SHALL use **Axum** and **Tokio**.
   - The web interface (HTML5, CSS Glassmorphism, Canvas 2D AS-PATH topology, and multilingual i18n)
     SHALL be embedded directly into the binary using compiled templates (e.g., `askama`) or `include_str!`.
   - Node.js, Next.js build steps, and Python runtimes SHALL NOT exist in the final deployment.

2. **No Mandatory External Web Server (No Nginx / Apache Required):**
   - The binary SHALL be capable of serving HTTP/HTTPS directly on ports 80/443 with built-in TLS termination
     using **`rustls`** (or Let's Encrypt / ACME integration).
   - Operators MAY still place an external proxy (Cloudflare, Traefik, HAProxy) in front if their corporate policy
     dictates, but the Looking Glass MUST function standalone with zero external dependencies.

3. **Rebuilding What We Need from Netmiko in Native Rust:**
   - Router SSH communication SHALL use **`russh`** (asynchronous, memory-safe SSH in pure Rust).
   - The vendor-specific intelligence of Netmiko (terminal pagination disabling, prompt regex detection,
     and command formatting) SHALL be ported directly into Rust modules implementing the `VendorDriver` trait.
   - The initial core drivers SHALL include:
     - `mikrotik`: MikroTik RouterOS v6 / v7
     - `huawei`: Huawei VRP (e.g. `screen-length 0 temporary`)
     - `cisco`: Cisco IOS-XR and IOS-XE (`terminal length 0`)
     - `juniper`: Juniper Junos (`set cli screen-length 0`, optional `| display json`)
     - `datacom`: Datacom DmOS (`terminal length 0`)
     - `nokia`: Nokia SR OS (`environment no more`)
     - `bird`: BIRD 2 (via Unix Domain Socket / SSH)

```mermaid
flowchart TB
    visitor([Visitor Browser])
    
    subgraph appliance [Single Standalone Looking Glass Binary - Rust]
        tls[Built-in TLS & HTTP/2 Engine<br/>rustls + Axum]
        ui[Embedded Web UI<br/>HTML5 + CSS + Canvas AS-PATH Topology<br/>pt / en / es i18n]
        api[Query API & SSE Streamer]
        validator[Strict Input Validation<br/>ipnet + std::net]
        limiter[Tokio Concurrency Semaphore<br/>per-router rate limiter]
        
        subgraph drivers [Native Lightweight Vendor Drivers]
            d_mtk[MikroTik Driver]
            d_hw[Huawei Driver]
            d_cs[Cisco Driver]
            d_jn[Juniper Driver]
            d_dc[Datacom Driver]
            d_nk[Nokia Driver]
            d_bd[BIRD Driver]
        end
        
        ssh_pool[Asynchronous russh Engine]
    end

    subgraph routers [Operator Network]
        r1[(MikroTik)]
        r2[(Huawei)]
        r3[(Cisco)]
        r4[(Juniper)]
    end

    visitor -->|HTTPS :443<br/>No Nginx/Apache needed| tls
    tls --> ui
    tls --> api
    api --> validator
    validator --> limiter
    limiter --> drivers
    drivers --> ssh_pool
    ssh_pool -->|Direct Async SSH| r1
    ssh_pool -->|Direct Async SSH| r2
    ssh_pool -->|Direct Async SSH| r3
    ssh_pool -->|Direct Async SSH| r4
```

### 4. Internationalisation inside the binary

ADR-0004 required three languages and a locale prefix in the URL. Removing
Next.js does not remove that requirement, so it is restated here in terms of
this architecture:

1. Locales SHALL remain `pt-BR`, `en` and `es`, served under `/pt`, `/en` and
   `/es` by Axum routes. A request without a prefix SHALL be redirected using
   `Accept-Language`, falling back to the operator's configured default.
2. Message catalogues SHALL live in versioned files (one per locale) and SHALL
   be embedded at compile time. Strings MUST NOT be hardcoded in templates or
   in Rust.
3. `scripts/check-i18n.sh` SHALL be pointed at those catalogues, so a key that
   exists in one locale and not the others fails CI, exactly as before.
4. English is the source locale; a missing translation SHALL fall back to
   English rather than rendering a raw key.
5. Router output, router names, POP names and technical identifiers MUST NOT be
   translated.

### 5. Building the interface

1. The interface MAY be authored with a front-end toolchain, but the published
   artefact SHALL be static files embedded in the binary. Node MUST NOT be
   required to run NOGGlass, and MUST NOT appear in the runtime image.
2. The topology graph and the design tokens follow
   [ADR-0008](0008-frontend-design-system-and-topology-graph.md).
3. Templates and assets SHALL be embedded with `include_str!`, `rust-embed` or
   `askama`, so a single binary remains the deliverable.

## Consequences

> Accepted on 2026-09-20 together with [ADR-0005](0005-backend-implementation-language.md).
> [ADR-0002](0002-fastapi-backend-and-direct-ssh.md) is fully superseded by this
> document; [ADR-0004](0004-web-interface-internationalisation.md) is superseded
> only in how the interface is delivered — its locale rules are restated above
> and still apply. Exposure and packaging follow
> [ADR-0012](0012-standalone-binary-exposure-and-packaging.md), not the ports
> described here.

### Positive
- **Truly Boring Infrastructure:** One executable, one config file, zero external runtimes (no Node, no Python, no Nginx, no Apache).
- **Extreme Efficiency:** Total RAM footprint under 35 MB under heavy load. Boots in milliseconds.
- **Enterprise Security:** Memory-safety by design. Immunity to Python dependency supply-chain attacks and zero shell-escape injection vulnerabilities.
- **Direct Streaming:** Real-time Server-Sent Events (SSE) fed directly from router SSH buffers to the visitor browser with sub-millisecond dispatch latency.

### Negative / Trade-offs
- The project maintainers write and maintain the Rust vendor drivers and web templates rather than relying on off-the-shelf Python libraries.
- Adding a new vendor requires implementing the Rust `VendorDriver` trait instead of dropping in an unvetted Python script.
