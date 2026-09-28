# Verifying NOGGlass's own BGP session

## TL;DR

A reproducible harness for issue #171: an FRR peer that NOGGlass dials over BGP
and that advertises two documentation prefixes. Bring FRR up on a docker
network, run NOGGlass with the `[bgp]` section here, and confirm its local RIB
fills — the log goes `bgp: local RIB prefixes=0` → `prefixes=2` and
`GET /api/bgp/route?target=203.0.113.0/24` returns the path. See **Known
blocker** before expecting routes: with `netgauze-bgp-speaker` 0.13.0 the
session establishes but resets on the first UPDATE (a one-line upstream bug);
routes flow once that is fixed, which was verified locally.

## Files

- `frr.conf`, `daemons`, `vtysh.conf` — an FRR speaker: AS 65001, router-id
  172.30.0.2, peering with NOGGlass at 172.30.0.1, advertising 203.0.113.0/24
  and 198.51.100.0/24.
- `nogglass.toml` — a minimal inventory whose `[bgp]` section dials that FRR.

## Steps

```bash
# 1. A docker network and the FRR peer on it.
docker network create --subnet 172.30.0.0/24 ngbgp
docker run -d --name ngbgp-frr --network ngbgp --ip 172.30.0.2 --privileged \
  -v "$PWD/lab/localbgp/frr.conf:/etc/frr/frr.conf" \
  -v "$PWD/lab/localbgp/daemons:/etc/frr/daemons" \
  -v "$PWD/lab/localbgp/vtysh.conf:/etc/frr/vtysh.conf" \
  quay.io/frrouting/frr:9.1.0

# 2. NOGGlass, dialing the FRR peer. Its source on the bridge is 172.30.0.1,
#    which is the neighbour FRR expects.
NOGGLASS_CONFIG=lab/localbgp/nogglass.toml NOGGLASS_HTTP_ADDR=127.0.0.1:18080 \
  NOGGLASS_LOG=info ./target/debug/nogglass

# 3. Confirm. Within ~30s the log prints `bgp: local RIB prefixes=2`, and:
curl -s 'http://127.0.0.1:18080/api/bgp/route?target=203.0.113.0/24'

# Tear down.
docker rm -f ngbgp-frr && docker network rm ngbgp
```

To watch what the engine parses (useful for the blocker below), run NOGGlass
with `NOGGLASS_LOG="info,netgauze_bgp_pkt=trace"`.

## Known blocker

With `netgauze-bgp-speaker` 0.13.0 the session reaches Established and then
resets ~1s later on `UpdateMsgErr → MissingWellKnownAttribute(NEXT_HOP)`. The
UPDATE is well-formed — NEXT_HOP is present — but the speaker's post-decode
mandatory-attribute check breaks out of its loop once ORIGIN and AS_PATH are
seen, so with the standard attribute order (ORIGIN, AS_PATH, NEXT_HOP, …) it
never observes NEXT_HOP. It is a one-line upstream fix; with it applied, this
harness shows the RIB reaching two prefixes. The pre-authorised Rotonda engine
([ADR-0016](../../docs/adr/0016-one-rust-engine-for-bgp-session-and-bmp.md))
was confirmed to ingest the same routes. Engine resolution is tracked on #171.
