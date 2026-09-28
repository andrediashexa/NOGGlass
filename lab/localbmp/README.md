# Verifying NOGGlass's BMP station

## TL;DR

A reproducible harness for issue #172: two FRR routers where one (**A**)
originates two documentation prefixes and advertises them to the other (**B**),
and **B** exports its post-policy Adj-RIB-In to NOGGlass over BMP. Bring the pair
up on a docker network, run NOGGlass with the `[bmp]` section here, and confirm
its RIB fills — the log goes `bmp: station RIB prefixes=0` → `prefixes=2` and
`GET /api/bmp/route?target=203.0.113.0/24` returns the path. The bytes this
harness produced were captured and frozen as the unit-test fixture
`crates/looking-glass-core/src/bmp/testdata/frr-9.1-bmp-stream.bin`, so the
end-to-end decode is proven in CI without docker.

## Files

- `frr-a.conf` — FRR speaker A: AS 65002, router-id 172.30.0.2, originates
  203.0.113.0/24 and 198.51.100.0/24, advertises them to B.
- `frr-b.conf` — FRR speaker B: AS 65003, router-id 172.30.0.3, peers with A and
  exports its **post-policy** Adj-RIB-In to NOGGlass at 172.30.0.1:11019.
- `daemons` — enables `bgpd` with the **`-M bmp`** module (BMP is a bgpd module
  in FRR; without it the `bmp targets` config is silently rejected).
- `nogglass.toml` — a minimal inventory whose `[bmp]` station listens on
  0.0.0.0:11019 and allow-lists B at 172.30.0.3.

## Steps

```bash
# 1. A docker network and the two FRR speakers on it.
docker network create --subnet 172.30.0.0/24 ngbmp
for n in a b; do
  docker run -d --name ngbmp-$n --network ngbmp --ip 172.30.0.$([ $n = a ] && echo 2 || echo 3) \
    --privileged \
    -v "$PWD/lab/localbmp/frr-$n.conf:/etc/frr/frr.conf" \
    -v "$PWD/lab/localbmp/daemons:/etc/frr/daemons" \
    -v "$PWD/lab/localbmp/vtysh.conf:/etc/frr/vtysh.conf" \
    quay.io/frrouting/frr:9.1.0
done

# 2. NOGGlass, its BMP station listening on the bridge so B can reach it at the
#    gateway 172.30.0.1. B connects from 172.30.0.3, which the allow-list expects.
NOGGLASS_CONFIG=lab/localbmp/nogglass.toml NOGGLASS_HTTP_ADDR=127.0.0.1:18081 \
  NOGGLASS_LOG=info ./target/debug/nogglass

# 3. Confirm. Within ~30s the log prints `bmp: station RIB prefixes=2`, and:
curl -s 'http://127.0.0.1:18081/api/bmp/route?target=203.0.113.0/24'

# Tear down.
docker rm -f ngbmp-a ngbmp-b && docker network rm ngbmp
```

## Why post-policy

`frr-b.conf` uses `bmp monitor ipv4 unicast post-policy`, not pre-policy. FRR
does **not** retain a peer's pre-policy Adj-RIB-In unless
`neighbor <peer> soft-reconfiguration inbound` is set, so `pre-policy`
monitoring streams only End-of-RIB markers (empty UPDATEs) and the station RIB
stays empty — which looks like a bug but is FRR discarding the pre-policy table.
Post-policy monitors the routes as installed after inbound policy, which for
this accept-all lab is both prefixes, and needs no extra memory. Either works;
post-policy is the lighter default. A monitored route arrives keyed by the
neighbour that sent it (172.30.0.2), exactly as the normalised model expects.

## What this proved

The station accepts B (allow-list), decodes FRR's real BMP stream with NetGauze's
`BmpCodec`, and fills the RIB: `GET /api/bmp/route` returns both prefixes keyed
by 172.30.0.2. Unlike the local BGP session (#171), the BMP path does not hit the
NetGauze speaker bug — it uses the packet parser/codec directly. The captured
stream is frozen as a unit-test fixture, so this is a permanent regression test.
