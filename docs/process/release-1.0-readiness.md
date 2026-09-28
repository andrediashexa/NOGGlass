# 1.0.0 readiness

## TL;DR

NOGGlass is functionally a 1.0 looking glass — the SSH engine, the typed BGP
model, the AS-PATH graph, tiered RPKI, the global view, rate limiting and the
i18n UI are all in place and green in CI. What stands between here and a **1.0.0**
tag is not more code: it is **verifying the four vendor drivers that have never
run against a real device** (Juniper Junos, Nokia SR OS, Datacom DmOS, MikroTik
v6 — each blocked on external access), and **two product decisions** the
maintainers own (federation scope, [#78]; and whether the routing engine removed
in [#194] is in or out of 1.0). 1.0.0 is a deliberate stability commitment, cut
with `Release-As: 1.0.0`, not an automatic bump.

The key words "MUST", "SHOULD" and "MAY" are used as in
[RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

## 5W2H

| Question | Answer |
|---|---|
| **What** | The checklist between the current release and a 1.0.0 tag. |
| **Why** | 1.0.0 is a public stability promise; it MUST be earned, not marketed. |
| **Who** | Maintainers decide scope and provide device access; contributors verify drivers and cut the release. |
| **Where** | Vendor drivers and `lab/`, `docs/reference/vendor-support.md`, the release tooling (release-please). |
| **When** | Before tagging 1.0.0; no fixed date. |
| **How** | Verify each unverified driver against a real device (ADR-0015), settle the open decisions, then `Release-As: 1.0.0`. |
| **How much** | Mostly external cost: licences, runnable images, or hardware for the four unverified vendors. The rest is decision and release mechanics. |

## Context

A 1.0.0 release says the configuration surface, the API and the behaviour are
stable enough to depend on. Two project rules shape what that requires here:

- **A driver counts only when it has run against a real device**
  ([ADR-0015](../adr/0015-vendor-drivers-are-verified-against-real-devices.md)).
  The project's own history is that every driver written from documentation was
  wrong once a real device ran it, and none failed loudly. So an *unverified*
  driver MUST NOT be presented as production-ready in a 1.0.
- **Releases are automated** ([ADR-0014](../adr/0014-release-every-change-with-patch-prereleases.md))
  via release-please and Conventional Commits. A major bump to 1.0.0 is a
  deliberate act (`Release-As: 1.0.0`), not something a normal commit triggers.

The embedded BGP session and BMP station ([#171]/[#172]) were removed in [#194];
the agreed direction is to bring the routing engine back as a **separate
container** ([ADR-0016] is marked reverted). Whether that lands before 1.0 or
after is one of the open decisions below. Everything deferred past 1.0 — the
routing engine, a web admin interface, federation — lives on the
[roadmap](roadmap.md).

## Details

```mermaid
flowchart TB
    subgraph done["Done — green in CI"]
        ssh["SSH engine + typed BGP model"]
        graph["AS-PATH graph, RPKI, global view"]
        ui["i18n UI, rate limiting, security headers"]
        five["5 drivers verified end to end"]
    end
    subgraph gate["Gate to 1.0.0"]
        verify["Verify 4 unverified drivers<br/>(external access needed)"]
        decide["2 product decisions<br/>(#78 federation, engine scope)"]
    end
    cut["Release-As: 1.0.0"]
    done --> gate
    verify --> cut
    decide --> cut
```

### 1. Vendor verification — the main gate

From [`vendor-support.md`](../reference/vendor-support.md):

| Driver | Status | What it needs to reach 1.0 |
|---|---|---|
| Huawei VRP, Cisco IOS-XE, Cisco IOS-XR, MikroTik v7, BIRD | ✅ Verified end to end | Nothing — done. |
| Arista EOS, FRR | 🟡 Supported (compatible CLI) | SHOULD be run once against a real device to promote to verified. |
| Juniper Junos | ⚠️ Unverified | A vMX/vSRX image that runs BGP. |
| Nokia SR OS | ⚠️ Unverified | The TiMOS licence ([#156]). |
| Datacom DmOS | ⚠️ Unverified | Physical hardware (cannot be virtualised). |
| MikroTik v6 | ⚠️ Unverified | A CHR image that boots under vrnetlab. |

Each unverified driver MUST either be verified against its device, or the 1.0
release notes and the README MUST scope the production claim to the verified set
and mark the rest clearly (the README already carries the per-vendor status).

### 2. Product decisions (maintainers)

- **Federation scope** ([#78]): decide whether the authenticated, multi-tenant
  platform ([ADR-0011]) is in 1.0. If not, document it as post-1.0 and close the
  issue.
- **Routing engine scope**: decide whether the BGP session / BMP station return
  (as a separate container) before 1.0, or whether 1.0 ships as the SSH looking
  glass and the engine is a post-1.0 feature.

### 3. Release mechanics

- The removal in [#194] makes the next release **0.8.0** (release-please PR
  already open). That MAY be cut at any time.
- 1.0.0 is then a deliberate `Release-As: 1.0.0` once the gate above is met.
- Before tagging, confirm CI is green, `docs/reference/vendor-support.md` is
  current, and the README's claims match the verified set.

## Consequences

- **Honest 1.0:** the product ships claiming only what has been proven, which is
  the whole point of ADR-0015.
- **Blocked on access, not code:** the remaining engineering is small; the real
  cost is device/licence access for four vendors, which is the maintainers' to
  provide or to scope out.

[#78]: https://github.com/andrediashexa/NOGGlass/issues/78
[#156]: https://github.com/andrediashexa/NOGGlass/issues/156
[#171]: https://github.com/andrediashexa/NOGGlass/issues/171
[#172]: https://github.com/andrediashexa/NOGGlass/issues/172
[#194]: https://github.com/andrediashexa/NOGGlass/issues/194
[ADR-0011]: ../adr/0011-authenticated-collaborative-multitenant-looking-glass.md
[ADR-0016]: ../adr/0016-one-rust-engine-for-bgp-session-and-bmp.md
