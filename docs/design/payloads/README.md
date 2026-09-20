# Captured payloads

<!-- lint: no-diagram -->
<!-- These are captures, not a document: there is no flow to draw. -->

## TL;DR

Every file here is a real response from a running NOGGlass, captured against the
mock router so the values are safe to publish. They exist so an interface can be
designed and checked against what the product actually returns, rather than
against an idea of it.

## 5W2H

| Question | Answer |
|---|---|
| **What** | One JSON file per screen and per state the interface has to render. |
| **Why** | A mock-up built on invented data hides exactly the cases that matter: absent values, failed lookups, sessions that are down. |
| **Who** | Whoever designs the interface; whoever writes the rendering code. |
| **Where** | `docs/design/payloads/`. |
| **When** | Captured from `v0.4.2`. Re-capture when the API changes. |
| **How** | `curl` against `/api/query` and `/api/routers` on an instance with the mock router enabled. |
| **How much** | No cost; the files are a few kilobytes. |

## The files

| File | The state it shows |
|---|---|
| `routers.json` | The selector: names, locations, which queries each router answers |
| `bgp_route.json` | Two paths, one best, RPKI valid, communities, an alternative with **no MED** |
| `bgp_route_rpki_invalid.json` | RPKI **invalid** with a blackhole community — the alarming case |
| `bgp_route_no_route_global_seen.json` | The router has **no route**, while the Internet announces the prefix |
| `bgp_summary.json` | Three sessions, one of them `Idle` |
| `traceroute.json` | Four hops, the third silent |
| `ping.json` | A successful ping with timings |
| `error_invalid_target.json` | A refused target, with the stable code the interface translates |

The values every design has to handle are visible in these files: `null` for
what the router did not report, `not_checked` for RPKI nobody validated, and an
`agreement` state that is `unknown` when the global comparison could not run.

## Re-capturing

```bash
for q in ping traceroute bgp_route bgp_summary; do
  curl -s -X POST localhost:8080/api/query \
    -H 'content-type: application/json' \
    -d "{\"router\":\"demo\",\"type\":\"$q\",\"target\":\"198.51.100.10\"}" \
    | python3 -m json.tool > "$q.json"
done
```

Captures MUST come from the mock router. A capture from a real router would put
an operator's addresses and AS numbers in the repository.
