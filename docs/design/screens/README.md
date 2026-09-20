# Screens

<!-- lint: no-diagram -->
<!-- A screenshot index; the screens themselves are the illustration. -->

## TL;DR

Eleven screenshots of the running interface, one per state a design has to
handle. Captured at 1440 px wide against the mock router, so every address and
AS number is from a documentation range and safe to publish.

## 5W2H

| Question | Answer |
|---|---|
| **What** | The interface as it looks today, state by state. |
| **Why** | A redesign needs the current thing to react to, including the states nobody thinks to mock up. |
| **Who** | Whoever designs the interface. |
| **Where** | `docs/design/screens/`. |
| **When** | Captured from `v0.4.2` with the shareable-link feature. |
| **How** | Headless Chromium against links that carry the query. |
| **How much** | About 1 MB of PNGs. |

## The screens

| File | What it shows |
|---|---|
| `01-inicial.png` | Empty state: the form, and the notice that this router is fabricated |
| `02-rota-bgp.png` | A BGP route: graph, path table, global view, router output. The second path has **no MED**, rendered as the "not reported" marker |
| `03-rpki-invalido.png` | RPKI **invalid** — a possible hijack, in red, with a blackhole community |
| `04-sem-rota-mundo-ve.png` | The router has no route while the Internet announces the prefix |
| `05-traceroute.png` | Hop list with a silent hop keeping its number |
| `06-sessoes-bgp.png` | Session table with one session `Idle`, in red |
| `07-ping.png` | A ping with its timings |
| `08-erro-alvo-invalido.png` | A refused target, translated from the error code |
| `09-ipv6-sem-roa.png` | IPv6 with no ROA — the third RPKI state |
| `10-english.png` | The same screen in English |
| `11-espanol.png` | The same screen in Spanish |

## Re-capturing

The interface takes the query in the URL, so a screenshot is a page load:

```bash
chromium --headless=new --window-size=1440,1500 \
  --screenshot=02-rota-bgp.png \
  "http://127.0.0.1:8080/pt/?router=demo&type=bgp_route&target=198.51.100.0%2F24"
```

Captures MUST use the mock router. A screenshot of a real router publishes an
operator's routing table.
