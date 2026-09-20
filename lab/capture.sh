#!/usr/bin/env bash
# Captures a node's raw output for every command in the catalogue.
#
# The point is the raw text. A fixture written from documentation is a guess
# about what a router prints; this is what one actually printed, and it is what
# a driver's tests should be written against.
#
# Usage:
#   ./capture.sh --node dut --vendor frr
#   ./capture.sh --node dut --vendor mikrotik_routeros --via ssh --user admin
#
# The output lands in captured/<vendor>/<query>.txt, one file per command, with
# the command itself on the first line as a comment.
#
# Credentials are never written anywhere. Pass a password in NOGGLASS_LAB_PASSWORD
# if the node needs one; it is read from the environment and not echoed.
set -euo pipefail

NODE=""
VENDOR=""
VIA="docker"
USER_NAME="admin"
PORT=22
LAB_PREFIX="clab-nogglass"
CATALOGUE="../crates/looking-glass-core/src/catalogue/commands.toml"
OUT_ROOT="captured"

# Targets that exist in this lab. A capture against anything else produces
# output that says "no route", which teaches a parser nothing.
TARGET_V4="203.0.113.10"
TARGET_V6="2001:db8:beef:f::10"
PREFIX_V4="203.0.113.0/24"
PREFIX_V6="2001:db8:beef::/48"
# The aggregate with the AS_SET, which is the interesting one.
PREFIX_ASSET="198.51.100.0/24"
ASN="64496"
COUNT=4

usage() { sed -n '2,18p' "$0"; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --node) NODE="$2"; shift ;;
    --vendor) VENDOR="$2"; shift ;;
    --via) VIA="$2"; shift ;;          # docker | ssh
    --user) USER_NAME="$2"; shift ;;
    --port) PORT="$2"; shift ;;
    --host) HOST_OVERRIDE="$2"; shift ;;
    --out) OUT_ROOT="$2"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

[[ -n "$NODE" ]]   || { echo "--node is required" >&2; exit 2; }
[[ -n "$VENDOR" ]] || { echo "--vendor is required (a section of commands.toml, or 'frr')" >&2; exit 2; }
[[ -f "$CATALOGUE" ]] || { echo "run this from lab/: $CATALOGUE not found" >&2; exit 2; }

container="${HOST_OVERRIDE:-${LAB_PREFIX}-${NODE}}"
out="$OUT_ROOT/$VENDOR"
mkdir -p "$out"

# FRR is not in the catalogue — it is the reference node, not a supported
# vendor — so its commands are listed here instead.
frr_commands() {
  cat <<CMDS
ping_v4	ping -c $COUNT $TARGET_V4
ping_v6	ping6 -c $COUNT $TARGET_V6
traceroute_v4	traceroute -n $TARGET_V4
traceroute_v6	traceroute6 -n $TARGET_V6
bgp_route_v4	vtysh -c "show bgp ipv4 unicast $PREFIX_V4"
bgp_route_asset	vtysh -c "show bgp ipv4 unicast $PREFIX_ASSET"
bgp_route_v6	vtysh -c "show bgp ipv6 unicast $PREFIX_V6"
bgp_route_asn	vtysh -c "show bgp ipv4 unicast regexp _${ASN}_"
bgp_summary	vtysh -c "show bgp ipv4 unicast summary"
bgp_summary_v6	vtysh -c "show bgp ipv6 unicast summary"
bgp_table	vtysh -c "show bgp ipv4 unicast"
CMDS
}

# Everything else comes out of the catalogue itself, so a capture runs exactly
# what the product would send — including the paging command, which changes the
# output more than anything else here.
catalogue_commands() {
  python3 - "$CATALOGUE" "$VENDOR" <<'PY'
import sys, tomllib

path, vendor = sys.argv[1], sys.argv[2]
with open(path, "rb") as handle:
    catalogue = tomllib.load(handle)

if vendor not in catalogue:
    sys.exit(f"{vendor} is not in the catalogue. Known: {', '.join(catalogue)}")

entry = catalogue[vendor]
values = {
    "target": None,          # filled per query below
    "count": "4",
    "asn": "64496",
}

targets = {
    "ping_v4": "203.0.113.3",
    "ping_v6": "2001:db8:beef::3",
    "traceroute_v4": "203.0.113.3",
    "traceroute_v6": "2001:db8:beef::3",
    "bgp_route_v4": "203.0.113.0/24",
    "bgp_route_asset": "198.51.100.0/24",
    "bgp_route_v6": "2001:db8:beef::/48",
}

# bgp_route_asset is not a catalogue key: it reuses the v4 template against the
# aggregate, because the AS_SET is the case worth capturing twice.
template_for = dict.fromkeys(targets, None)
for name in targets:
    template_for[name] = entry.get("bgp_route_v4" if name == "bgp_route_asset" else name)
template_for["bgp_route_asn"] = entry.get("bgp_route_asn")
template_for["bgp_summary"] = entry.get("bgp_summary")

if paging := entry.get("disable_paging"):
    print(f"disable_paging\t{paging}")

for name, template in template_for.items():
    if not template:
        continue
    target = targets.get(name, "")
    substitutions = dict(values)
    substitutions["target"] = target
    if "/" in target:
        network, _, length = target.partition("/")
        substitutions["network"] = network
        substitutions["prefix_len"] = length
        # Only correct for the /24 and /25 this lab announces, which is all a
        # capture needs; the product computes it properly.
        substitutions["netmask"] = {"24": "255.255.255.0", "25": "255.255.255.128"}.get(length, "255.255.255.0")
    else:
        substitutions["network"] = target
        substitutions["prefix_len"] = ""
        substitutions["netmask"] = ""
    try:
        print(f"{name}\t{template.format(**substitutions)}")
    except KeyError as missing:
        sys.exit(f"{name}: the catalogue uses a placeholder this script does not fill: {missing}")
PY
}

run_docker() {
  local command="$1"
  docker exec "$container" sh -c "$command" 2>&1
}

run_ssh() {
  local command="$1"
  # BatchMode so a node that wants a password fails loudly instead of hanging
  # on a prompt nobody is there to answer.
  if [[ -n "${NOGGLASS_LAB_PASSWORD:-}" ]]; then
    command -v sshpass >/dev/null 2>&1 || {
      echo "NOGGLASS_LAB_PASSWORD is set but sshpass is not installed" >&2
      exit 3
    }
    sshpass -e ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
      -o ConnectTimeout=10 -p "$PORT" "$USER_NAME@$container" "$command" 2>&1
  else
    ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
      -o BatchMode=yes -o ConnectTimeout=10 -p "$PORT" "$USER_NAME@$container" "$command" 2>&1
  fi
}

# sshpass reads the password from SSHPASS, so the value never appears in a
# command line where `ps` would show it.
export SSHPASS="${NOGGLASS_LAB_PASSWORD:-}"

echo "Capturing $VENDOR from $container over $VIA"

if [[ "$VENDOR" == "frr" ]]; then
  commands="$(frr_commands)"
else
  commands="$(catalogue_commands)"
fi

failures=0
while IFS=$'\t' read -r name command; do
  [[ -n "$name" ]] || continue
  printf '  %-18s %s\n' "$name" "$command"
  if [[ "$VIA" == "ssh" ]]; then
    output="$(run_ssh "$command" || true)"
  else
    output="$(run_docker "$command" || true)"
  fi
  if [[ -z "$output" ]]; then
    echo "    nothing came back — recorded as empty, which is itself a finding"
    failures=$((failures + 1))
  fi
  {
    printf '! captured from %s (%s) on %s\n' "$container" "$VENDOR" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '! command: %s\n' "$command"
    printf '!\n'
    printf '%s\n' "$output"
  } > "$out/$name.txt"
done <<< "$commands"

echo
echo "Written to $out/:"
ls -1 "$out"
if [[ $failures -gt 0 ]]; then
  echo
  echo "$failures command(s) produced no output. Check that BGP has converged:"
  echo "  docker exec $container vtysh -c 'show bgp ipv4 unicast summary'"
fi
