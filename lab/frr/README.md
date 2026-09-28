# Verifying the FRR driver

## TL;DR

A reproducible harness for the `frr` driver ([ADR-0015](../../docs/adr/0015-vendor-drivers-are-verified-against-real-devices.md)):
two FRR 9.1.0 speakers over eBGP, so **B** learns **A**'s routes and prints the
`show ip bgp` table, a detail block, and the summary a real device produces. The
captured output is frozen as the driver's fixtures
(`crates/looking-glass-core/src/vendors/testdata/frr-9.1-bgp-*.txt`), which is
what promoted FRR from *Supported* to *Verified* — a real route with an **empty
LocPrf** column had to parse without swallowing the AS as the weight.

## 5W2H

| Question | Answer |
|---|---|
| **What** | How the `frr` driver was verified against a real FRR. |
| **Why** | A driver only counts when it has read a real device (ADR-0015). |
| **Who** | Maintainers, when re-verifying or bumping the FRR version. |
| **Where** | This directory; the fixtures beside the parser; the tests in `frr.rs`. |
| **When** | Re-run on an FRR version bump. |
| **How** | Two FRR containers on a docker network; capture over `vtysh`. |
| **How much** | Free — FRR is open source and containerised. |

## Steps

```bash
docker network create --subnet 172.31.0.0/24 ngfrr
for n in a b; do
  ip=$([ $n = a ] && echo 2 || echo 3)
  docker run -d --name ngfrr-$n --network ngfrr --ip 172.31.0.$ip --privileged \
    -v "$PWD/lab/frr/frr-$n.conf:/etc/frr/frr.conf" \
    -v "$PWD/lab/frr/daemons:/etc/frr/daemons" \
    -v "$PWD/lab/frr/vtysh.conf:/etc/frr/vtysh.conf" \
    quay.io/frrouting/frr:9.1.0
done

# Once B<->A is Established, capture what a real device prints:
docker exec ngfrr-b vtysh -c "show ip bgp summary"
docker exec ngfrr-b vtysh -c "show ip bgp"
docker exec ngfrr-b vtysh -c "show ip bgp 198.51.100.0/24"

docker rm -f ngfrr-a ngfrr-b && docker network rm ngfrr
```

## Files

- `frr-a.conf` — AS 65100, originates 198.51.100.0/24 and 203.0.113.0/24.
- `frr-b.conf` — AS 65001, peers with A and receives its routes.
- `daemons`, `vtysh.conf` — enable `bgpd`/`zebra` and integrated config.
