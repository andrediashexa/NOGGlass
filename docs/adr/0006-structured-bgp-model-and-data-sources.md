# ADR-0006 — Structured BGP model, data sources and RIPE cross-check

- **Status:** Accepted
- **Date:** 2026-09-17
- **Deciders:** André Dias, Marcelo Gondim

## TL;DR

The result of a BGP query is a normalised path model, not vendor text. Data is
collected per router — native structured output where the vendor offers it,
TextFSM-style parsing where it does not — and enriched with RPKI state and a
cross-check against RIPEstat, so the interface can show what the router sees
next to what the Internet sees. The AS-PATH topology graph is a core feature of
`0.1.0`, not a later addition, and Huawei VRP is the first vendor.

## 5W2H

| Question | Answer |
|---|---|
| **What** | The data model behind every BGP query, where the data comes from, and what enriches it. |
| **Why** | A topology graph, RPKI badges and community tags are impossible over raw vendor text; and an operator needs to compare their view with the global view. |
| **Who** | Backend and driver contributors; the frontend consumes the model as a contract. |
| **Where** | The query API, the vendor drivers and the enrichment layer. |
| **When** | Baseline for `0.1.0`; supersedes the deferrals listed in ADR-0002. |
| **How** | Normalised path model, per-router collection, optional external enrichment with caching. |
| **How much** | One extra HTTP call per query when enrichment is enabled, cached; no extra load on routers. |

## Context

ADR-0002 assumed raw text output with parsing deferred, and listed RPKI and a
topology graph as out of scope. A prototype built for one of the maintainers
(NOGGlass) demonstrated that the graph, the RPKI badge and the decoded
communities are the reason an operator would choose this Looking Glass over the
dozens that already print CLI text. That reverses the priority: the structured
model is the product.

Three collection strategies were weighed. A **BGP collector** of our own
(BIRD or GoBGP peering with the routers) yields uniform structured data for
every vendor at no cost to the routers, but it shows the collector's view rather
than the router's, and requires the operator to configure a session. **Native
APIs** give clean data where they exist. **Text parsing** is unavoidable for
Huawei VRP, Datacom DmOS, MikroTik RouterOS and BIRD, which are precisely the
platforms of the target market.

The decision keeps the router as the source of truth, because an operator
debugging traffic engineering needs the view of the specific router they chose.
A collector remains an attractive future addition and MUST get its own ADR.

## Decision

### 1. Normalised path model

Every driver SHALL return the same model, whatever the vendor:

| Field | Notes |
|---|---|
| `prefix` | Validated network object, never a raw string |
| `paths[]` | One entry per path in the RIB |
| `paths[].as_path` | List of AS numbers, ordered |
| `paths[].next_hop`, `paths[].peer` | Addresses as typed values |
| `paths[].is_best`, `paths[].is_valid` | Selection flags |
| `paths[].local_pref`, `med`, `weight`, `origin` | Selection attributes, nullable |
| `paths[].communities` | Standard, extended and large, raw plus decoded |
| `paths[].rpki` | `valid`, `invalid`, `not_found` or `unknown` |
| `origin_as` | Origin AS of the path |
| `raw` | The vendor output, always preserved and always available in the UI |

Fields the vendor does not provide MUST be `null`. A driver MUST NOT invent a
value, and the UI MUST render a missing attribute as unknown rather than as a
default.

### 2. Collection strategy

```mermaid
flowchart TB
    q[Query: prefix + router] --> drv{Driver capability}
    drv -->|native structured| json["Junos display json<br/>EOS eAPI, NX-OS json<br/>IOS-XR json, FRR json"]
    drv -->|text only| tfsm["VRP, DmOS, RouterOS, BIRD<br/>template parsing"]
    json --> norm[Normalised path model]
    tfsm --> norm
    norm --> enrich{Enrichment enabled?}
    enrich -->|yes| ext["RPKI state<br/>AS names<br/>RIPEstat cross-check"]
    enrich -->|no| out
    ext --> out[API response]
    out --> graph[AS-PATH topology graph]
    out --> table[Route table and attribute panels]
```

1. A driver SHALL prefer the vendor's structured output when it exists.
2. A driver MUST fall back to template-based parsing otherwise, and the
   template MUST live in a data file, not in code.
3. Parsing failure MUST degrade to the raw output with a clear warning, never to
   a wrong graph. A partially parsed result MUST be flagged as such.
4. Queries MUST NOT be spread across routers implicitly: one query, one router,
   chosen by the visitor.

### 3. Enrichment and the RIPE cross-check

1. RPKI validation state SHALL be resolved through a validator the operator
   configures. A local validator (Routinator, `rpki-client`) SHOULD be
   preferred, with a public API as fallback.
2. The interface SHALL offer a cross-check of the queried prefix against
   RIPEstat (`routing-status`, `announced-prefixes`, `rpki-validation`,
   `as-overview`), so "what this router sees" sits next to "what the Internet
   sees". This surfaces route leaks, hijacks and announcements that never
   propagated.
3. AS numbers SHOULD be resolved to names for the graph, from the same source.
4. Enrichment MUST be cached, MUST have a short timeout, and MUST NOT block the
   router result: the graph renders first and enrichment fills in.
5. Enrichment sends the queried prefix to a third party. It SHALL be
   configurable, its default SHALL be documented, and the privacy consequence
   MUST be stated in the deployment guide.
6. An enrichment failure MUST NOT fail the query.

### 4. Scope of `0.1.0`

1. Query types: `ping`, `traceroute`, `bgp_route` and `bgp_summary`.
2. Result views: streamed text for `ping` and `traceroute`; path model, route
   table, attribute panels and AS-PATH graph for the BGP queries.
3. First vendor: **Huawei VRP**, chosen deliberately because it has no
   structured output — an architecture that survives VRP survives the JSON
   vendors.
4. A mock driver SHALL exist from the start, serving RFC 5737 addresses and
   RFC 6996 private AS numbers, used by CI and by the public demo.
5. Documentation and fixtures MUST use only RFC 5737, RFC 3849 and RFC 6996
   ranges. Real customer prefixes MUST NOT appear in the repository.

## Consequences

- Adding a vendor means writing a template and a mapping, and the graph works
  for it automatically. That is the intended leverage.
- Vendors without structured output will lag on attribute completeness, and the
  UI MUST make that visible instead of hiding it.
- An external enrichment dependency enters the product, with caching, timeouts
  and a privacy note as the price.
- The BGP collector option stays open and would remove per-vendor parsing for
  route data; it MUST be decided in its own ADR before any code.
- ADR-0002 is partially superseded: RPKI and the topology graph are no longer
  deferred, and native structured APIs are now preferred over text where
  available.
