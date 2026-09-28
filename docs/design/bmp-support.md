# BMP support

## TL;DR

This document plans NOGGlass acting as a **BMP station** (RFC 7854): the
operator's real routers open a TCP session *to* NOGGlass and stream what their
BGP is doing — per-peer routes (pre/post-policy), session up/down and stats —
and NOGGlass serves that live view through the existing API/UI. BMP is
monitoring-only by definition, so it fits the read-only ethos exactly; but the
feed carries no authentication and NOGGlass MUST treat it as untrusted input and
parse it fail-closed. The open decision is **how the station is built** — native
Rust with a BGP/BMP parsing crate, or an external collector — recorded in an ADR
before code lands. Tracked in issue #172.

The key words "MUST", "MUST NOT", "SHOULD", "RECOMMENDED" and "MAY" are to be
interpreted as in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

## 5W2H

| Question | Answer |
|---|---|
| **What** | A BMP station in NOGGlass that accepts sessions initiated by monitored routers and turns their BMP stream (Initiation, Peer Up/Down, Route Monitoring, Stats, Termination) into the normalised path and summary models ([ADR-0006](../adr/0006-structured-bgp-model-and-data-sources.md)). |
| **Why** | BMP gives the looking glass a continuous, per-peer view of a real router's Adj-RIB-In (and Loc-RIB, RFC 9069) with no login and no CLI to scrape — the always-on complement to on-demand SSH and to a NOGGlass-held BGP session. |
| **Who** | Operators who point their routers' BMP exporters at NOGGlass; maintainers who choose the implementation and own the ADR. |
| **Where** | A new component and an inbound TCP listener (the RFC fixes no port; e.g. 11019). Configuration under `NOGGLASS_*`. The SSH drivers do not change. |
| **When** | After the implementation decision (ADR-00YY). This document is the plan and the discussion. |
| **How** | A BMP listener accepts router-initiated sessions, parses BMP + embedded BGP UPDATEs into `BgpPath`/summary, keeps per-peer RIBs, and serves them. |
| **How much** | One listener plus per-peer RIB state (memory scales with peers × table size); measured `mem_limit`. Engineering: the listener, a BMP+BGP parser, and per-peer RIB bookkeeping. |

## Context

BMP is a receive-and-observe protocol: the station never sends routing state to
a router, so it sits comfortably inside the read-only rule. The requirements
that still bite:

- The feature **MUST** default to disabled (`NOGGLASS_BMP_ENABLED=false`).
- BMP has **no authentication of its own** (RFC 7854 §3.2). NOGGlass **MUST**
  treat every session as untrusted: the listener **MUST** be reachable only from
  configured routers (ACL / private link), and TLS or a network tunnel is
  **RECOMMENDED** where the transport crosses untrusted ground.
- The parser **MUST** be defensive and fail closed: bounded reads, a malformed
  message drops the session rather than corrupting state, and an unparseable
  attribute is reported — never guessed, never `Ok(empty)` for failure. Absent
  attributes are `null`.
- Parsed routes and peer states **MUST** map into the normalised models with the
  raw form preserved, so a BMP-fed route reads like an SSH-scraped one.
- The inbound BMP listener is another **exception** to "no published ports
  except the proxy": opt-in, documented, firewalled to the router set.

## Details

```mermaid
flowchart LR
    r1[Router A\nBMP exporter]-->|BMP/TCP| st
    r2[Router B\nBMP exporter]-->|BMP/TCP| st
    subgraph NOGGlass
      st[BMP station\nlistener + FSM]-->par[BMP + BGP parser]
      par-->ribs[(Per-peer RIBs\npre / post-policy)]
      ribs-->api[Axum API]
      ssh[SSH drivers]-->api
    end
    api-->ui[UI / SSE]
    classDef ro fill:#efe;
    class st,par,ribs ro;
```

```mermaid
sequenceDiagram
    participant R as Monitored router
    participant S as BMP station
    participant P as Parser
    R->>S: TCP connect (router initiates)
    R->>S: Initiation
    R->>S: Peer Up (per monitored peer)
    R->>S: Route Monitoring (BGP UPDATE, pre/post-policy)
    P->>P: to BgpPath (raw kept); malformed -> drop session
    R->>S: Stats Report
    R->>S: Peer Down / Termination
    Note over S,R: Station only receives.\nIt never sends routing state back.
```

### The decision: how the station is built

To be settled in an ADR before implementation. The two Rust engines shortlisted
for the BGP session (issue #171) **also** speak BMP as a station, so the same
choice can serve both features — note this is the *station* (collector) role;
ordinary BGP speakers like GoBGP or BIRD are BMP *clients* (the monitored side)
and do **not** give us a collector. Candidates, Rust first:

| Approach | For | Against |
|---|---|---|
| **NetGauze** (`netgauze-bmp-service` + `netgauze-bmp-pkt`, BMP v3/v4) embedded | Keeps the single binary; typed end to end; an actor-based BMP receiver already exists; same family as the BGP-session option, so **one dependency covers #171 and #172**; Apache-2.0 | We own the per-peer RIB bookkeeping; young crate (2026) |
| **Rotonda** (NLnet Labs) as a sidecar | Purpose-built to open BMP (and BGP) sessions and collect routes into a RIB with a queryable JSON API; **also delivers #171**; NLnet Labs pedigree | A separate process, so not the single binary; pre-1.0 |
| **OpenBMP / obmp** as a sidecar | Battle-tested at scale | Heavy — a Kafka pipeline and a datastore; large operational surface |
| **pmacct `pmbmpd`** as a sidecar | Mature collector, lighter than OpenBMP | Still an external process and its output format to consume |

**Recommendation (open to discussion).** Embed **NetGauze**'s BMP crates in the
binary: the framing is small and well-scoped, typed parsing keeps us honest about
"never invent a value," staying in-process preserves the single-binary
deployment, and it is the same dependency family as the recommended BGP-session
engine — so #171 and #172 can be **one** decision. **Rotonda** (a Rust sidecar)
is the turnkey fallback that also covers both; the heavier external collectors
remain a **MAY** for very large fleets that already run one. All are pre-1.0, so
the ADR **MUST** pin a version.

No approach is chosen here. The table is the input to the ADR, which the
implementation blocks on. The decision is tracked in issue #172.

## Consequences

- **Easier:** a live, per-peer view of real routers with no credentials and no
  CLI; pre- and post-policy RIBs, which SSH rarely exposes cleanly; a natural
  base for history and change feeds later.
- **Harder:** an inbound listener that parses attacker-reachable input, so the
  parser's defensiveness is a security property, not a nicety; per-peer RIB
  memory grows with the fleet.
- **Ruled out:** any router-facing action. BMP is observe-only and so is this;
  there is no configuration that turns the station into a speaker.
- **Independent of the BGP session** (issue #171) as a *feature*: that is a
  peering NOGGlass runs, this is an inbound monitoring feed routers push, and
  neither blocks the other. As an *engine*, though, the two shortlisted Rust
  options do both, so they share the normalised model and **MAY** share one
  implementation.
