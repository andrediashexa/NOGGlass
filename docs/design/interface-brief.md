# How NOGGlass works, for whoever designs its interface

## TL;DR

NOGGlass answers four questions about a network, against a router the visitor
picks: can you reach this address, what path does traffic take, what does BGP
know about this prefix, and which sessions are up. The interface exists because
the answers arrive as structured data rather than as terminal text — every
screen below is a rendering of a typed payload, not of a CLI dump. A designer
working on this needs to know three things above all: which values can be
absent, which states are alarming, and that every visible string comes from a
translation catalogue in three languages.

## 5W2H

| Question | Answer |
|---|---|
| **What** | The product, its screens, its states and the data behind each one. |
| **Why** | So an interface can be designed from what the product actually returns, instead of from a guess about it. |
| **Who** | Whoever designs or redesigns the web interface, and the maintainers reviewing that work. |
| **Where** | The embedded interface at `/pt`, `/en` and `/es`; the payloads come from `/api/query`. |
| **When** | Current as of `v0.4.2`. |
| **How** | One page, a query form, and four result renderings that share a layout. |
| **How much** | No runtime cost: the page is static and embedded in the binary. |

## What a looking glass is for

An operator publishes a looking glass so that **people outside their network**
can see what their routers see. The visitor is usually one of three:

1. **A customer or a peer** asking "do you have a route to me, and does it look
   right". They want the answer in seconds, and they do not read CLI output.
2. **A network engineer at three in the morning**, comparing what this router
   sees with what their own router sees. They read CLI output fluently and want
   the raw text available, not hidden.
3. **A scraper**, which is why every query is rate limited and why nothing on
   the page reveals how to reach the routers.

The interface has to serve the first two without ever helping the third.

## The screen

One page. A form at the top, results below it, nothing else.

```mermaid
flowchart TB
    subgraph page [One page]
        bar["Top bar: brand · language switch"]
        form["Query form: router · query type · target · run"]
        notice["Notices: mock data · errors"]
        graph["AS-PATH graph — BGP route only"]
        table["Result table — varies by query"]
        global["Global view — BGP route, optional"]
        raw["Router output — always available"]
    end
    bar --> form --> notice --> graph --> table --> global --> raw
```

The order matters: the graph is the thing a visitor understands at a glance,
the table is what they read next, and the raw output is what an engineer scrolls
to. Moving the raw output above the table would serve the third audience at the
cost of the first two.

## The form

| Field | Behaviour |
|---|---|
| **Router** | Grouped by location. Each router is a name and a place, never an address. |
| **Query** | Only the query types that router answers. A router that does not offer traceroute does not show it. |
| **Target** | Free text, validated by the server. An address, a prefix in CIDR form, or an AS number. |

A router serving fabricated data shows a persistent notice. That is not
decoration: a demo instance that looks like a real one is a lie.

## The four results

### 1. BGP route — the reason the product exists

```json
{
  "kind": "bgp_route",
  "command": "show bgp 198.51.100.0/24",
  "duration_ms": 0,
  "result": {
    "paths": [
      {
        "prefix": "198.51.100.0/24",
        "next_hop": "192.0.2.254",
        "peer": "192.0.2.254",
        "is_best": true,
        "is_valid": true,
        "as_path": [65100, 65500],
        "local_pref": 150,
        "med": 10,
        "weight": 0,
        "origin": "igp",
        "communities": [
          { "raw": "65001:100", "kind": "standard", "name": null }
        ],
        "rpki": { "status": "valid", "source": "router" }
      }
    ],
    "raw_output": "…",
    "completeness": { "state": "complete" },
    "truncated": false
  },
  "agreement": { "state": "not_announced_globally" }
}
```

Rendered as three things: the **graph**, the **path table**, and the **global
view**.

**The graph** draws the local router on the left, the origin AS on the right,
transit in between. The best path is a solid line; alternatives are dashed. The
RPKI state sits under the origin AS, because that is what it describes — the
origin, not the path.

**Values that can be absent.** `local_pref`, `med`, `weight`, `origin`,
`next_hop`, `peer` and `prefix` are all nullable, and they are absent whenever
the router did not report them. This is the single most important rule in the
whole product: **a missing value MUST render as unknown, never as zero and never as
a blank cell that reads like zero**. A MED of 0 and a MED the router never
mentioned mean different things to an operator.

### 2. BGP summary — "which session is down"

```json
{
  "kind": "bgp_summary",
  "result": {
    "router_id": "192.0.2.1",
    "local_as": 65001,
    "peers": [
      { "peer_ip": "192.0.2.254", "peer_as": 65100, "state": "Established",
        "uptime": "05:12:33", "prefixes_received": 850000, "prefixes_accepted": null },
      { "peer_ip": "192.0.2.252", "peer_as": 65300, "state": "Idle",
        "uptime": "never", "prefixes_received": 0, "prefixes_accepted": null }
    ]
  }
}
```

A table of sessions. The session that is **not** established is the reason the
visitor ran this query, so it has to be findable without reading every row.
Everything else on this screen is context for that one row.

### 3. Traceroute — where the path stops

```json
{
  "kind": "traceroute",
  "result": {
    "target": "198.51.100.10",
    "hops": [
      { "hop": 1, "ip": "192.0.2.254", "hostname": "edge-01.example", "rtt_ms": [0.412, 0.398, 0.431] },
      { "hop": 3, "ip": null, "hostname": null, "rtt_ms": [] }
    ]
  }
}
```

A hop that did not answer keeps its number and shows that it was silent. It is
never dropped: dropping it renumbers the path and moves where the trace appears
to stop, which is the one thing this query is asked to show.

Each probe time is listed separately. Three probes of 1 ms, 1 ms and 400 ms tell
a story that an average of 134 ms erases.

### 4. Ping

```json
{
  "kind": "ping",
  "result": {
    "packets_sent": 5, "packets_received": 5, "packet_loss_percent": 0.0,
    "min_rtt_ms": 0.412, "avg_rtt_ms": 0.501, "max_rtt_ms": 0.688
  }
}
```

A total loss has **no** timings at all — the fields are null, not zero.

## States the design has to cover

These are not edge cases; every one of them happens on a normal day.

| State | What it means | Why it matters visually |
|---|---|---|
| **RPKI valid / invalid / no ROA / not checked** | Four states, not two | `invalid` is a possible hijack and has to be alarming; `not checked` must not look like a pass |
| **RPKI source: router / validator / RIPEstat / none** | Who said so | "My router filtered this" and "a public API says so" are different facts |
| **No route** | The router genuinely has no route | Different from an error, and different from a failed query |
| **Partial parse** | Some output was not understood | The parsed view is incomplete; the raw output is the fallback |
| **Truncated** | The answer hit the output cap | The visitor is seeing part of the answer |
| **Global view unavailable** | The comparison could not run | Must not read as agreement |
| **Origin mismatch** | The world sees a different origin AS | A hijack or a leak looks exactly like this |
| **Rate limited** | Too many queries from this visitor | Needs to say when to come back, not just refuse |
| **Busy** | The instance or the router is at its concurrency cap | Temporary, and worth saying so |
| **Router unreachable** | The session failed | Deliberately vague: it must not describe the operator's management network |
| **Mock data** | This router is fabricated | Persistent, not dismissible |

## Rules that constrain any design

These are requirements, not preferences: a design that breaks one of them
changes what the product is allowed to do.

1. **Nothing reveals the management plane.** No router address, port, username
   or credential MUST ever reach the browser. The router list carries a name, a
   vendor and a location.
2. **Every visible string MUST come from a catalogue** in `pt-BR`, `en` and `es`.
   Portuguese runs roughly 20% longer than English, and Spanish longer still, so
   no label may depend on fitting in a fixed width. A missing key falls back to
   English rather than showing the key.
3. **Errors MUST be translated from a stable code**, not from the server message.
   The codes are `target_unparseable`, `target_not_routable`, `prefix_too_short`,
   `host_bits_set`, `asn_out_of_range`, `unknown_router`, `query_not_offered`,
   `busy`, `rate_limited`, `timed_out`, `unreachable`, `unreadable`.
4. **Router output MUST stay available.** An engineer will want to read what the
   router actually printed; hiding it loses the second audience entirely.
5. **The command that ran is shown.** Operators ask for it, and it is built from
   a fixed template, not from what the visitor typed.
6. **No framework.** The page is embedded in a 13 MB binary and ships with it.
   A design that needs a build pipeline changes the product's deployment story.

## The current visual language

Defined in [ADR-0008](../adr/0008-frontend-design-system-and-topology-graph.md):
a dark operations-centre theme with neon accents.

| Token | Value | Used for |
|---|---|---|
| `--bg-base` | `#06090e` | Page background, with a faint grid |
| `--bg-card` | `rgba(13, 20, 31, 0.75)` | Panels, with a blur behind them |
| `--border-card` | `rgba(30, 58, 86, 0.6)` | Panel and table borders |
| `--accent-best` | `#00ff88` | Best path, RPKI valid, primary action |
| `--accent-backup` | `#00e5ff` | Alternative paths, focus ring |
| `--accent-local` | `#ff5722` | The querying router, mock-data notice |
| `--accent-transit` | `#c084fc` | Transit AS nodes, community chips |
| `--accent-invalid` | `#ff4d4d` | RPKI invalid, sessions that are down |
| `--text-main` | `#f8fafc` | Data |
| `--text-muted` | `#94a3b8` | Labels, unknown values |

Monospace for anything an operator would copy — addresses, AS numbers, metrics,
router output. Sans-serif for labels and navigation.

## What is deliberately open

A designer is not inheriting a finished product; these are unsettled:

- **Light theme.** There is none. A looking glass is often opened at 3 a.m.,
  which argues for dark-first, but nothing rules out a light variant.
- **The graph at scale.** It is drawn for a handful of paths. A prefix with
  twenty paths and long AS paths has not been designed for.
- **Mobile.** The page works at 400 px, but "works" means nothing overflows.
  The graph in particular is designed for a wide screen.
- **The relationship between the graph and the table.** They show the same
  paths. Selecting in one does not highlight the other, and it probably should.
- **Progressive results.** The API can stream stages over Server-Sent Events;
  the interface does not use it, so a slow query shows a disabled button and
  nothing else.
