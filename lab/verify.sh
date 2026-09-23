#!/usr/bin/env bash
# Checks a live router through the product, against everything that has gone
# wrong before.
#
#   ./verify.sh lab-ne40e
#   ./verify.sh lab-csr http://127.0.0.1:8080
#
# The router has to be configured in the running NOGGlass instance and wired
# into this lab's topology, because the checks are about what this lab
# announces: three paths for one prefix, one MED absent beside a zero and a
# hundred, a 32-bit ASN in asdot range, a session that never comes up, and a
# hop that stays silent.
#
# Every check here is a bug that shipped. None of them is "did it return 200";
# each one failed once with a plausible answer, which is the failure mode this
# product keeps finding (ADR-0015).
set -uo pipefail

ROUTER="${1:-}"
API="${2:-http://127.0.0.1:8080}"

if [[ -z "$ROUTER" ]]; then
  sed -n '2,16p' "$0"
  exit 2
fi

PREFIX_V4="203.0.113.0/24"
TARGET_V4="203.0.113.10"

passed=0
failed=0

pass() {
  printf '  \033[32mok\033[0m   %s\n' "$1"
  passed=$((passed + 1))
}

fail() {
  printf '  \033[31mFAIL\033[0m %s\n' "$1"
  printf '       expected: %s\n' "$2"
  printf '       got:      %s\n' "$3"
  failed=$((failed + 1))
}

query() {
  local type="$1" target="$2"
  curl -s -X POST "$API/api/query" \
    -H 'content-type: application/json' \
    -d "{\"router\":\"$ROUTER\",\"type\":\"$type\",\"target\":\"$target\"}" \
    --max-time 120
}

echo "Checking $ROUTER through $API"

# --- Sessions ---------------------------------------------------------------
echo
echo "bgp_summary"
summary="$(query bgp_summary "$TARGET_V4")"

read -r sessions zero_as established down <<<"$(
  printf '%s' "$summary" | python3 -c '
import json, sys
try:
    peers = json.load(sys.stdin).get("result", {}).get("peers", [])
except Exception:
    print("0 0 0 0")
    raise SystemExit
print(
    len(peers),
    sum(1 for p in peers if p.get("peer_as", 0) == 0),
    sum(1 for p in peers if p.get("state") == "Established"),
    sum(1 for p in peers if p.get("state") != "Established"),
)
'
)"

if [[ "${sessions:-0}" -ge 3 ]]; then
  pass "the router lists its sessions ($sessions)"
else
  fail "the router lists its sessions" "at least 3" "$sessions"
fi

# IOS-XR printed a speaker column where the AS was expected, and every peer
# came back as AS0.
if [[ "${zero_as:-1}" -eq 0 ]]; then
  pass "every session carries a real AS number"
else
  fail "every session carries a real AS number" "no peer with AS 0" "$zero_as of $sessions"
fi

# Huawei prints State and PfxRcd as separate columns, and a session in Idle was
# read as established — on the one screen whose job is showing what is down.
# RouterOS is the exception: it lists sessions, not configured neighbours, so a
# neighbour that never came up is absent rather than down.
if [[ "${down:-0}" -ge 1 ]]; then
  pass "a session that is not established is reported as such ($down)"
elif [[ "$sessions" -ge 6 ]]; then
  pass "no session is down, and this router lists only sessions (RouterOS)"
else
  fail "a session that is not established is reported as such" \
    "at least one, since the lab configures a neighbour that never answers" "none"
fi

# --- Route ------------------------------------------------------------------
echo
echo "bgp_route $PREFIX_V4"
route="$(query bgp_route "$PREFIX_V4")"

read -r paths best meds has_32bit <<<"$(
  printf '%s' "$route" | python3 -c '
import json, sys
try:
    paths = json.load(sys.stdin).get("result", {}).get("paths", [])
except Exception:
    print("0 0 - no")
    raise SystemExit
meds = sorted(("absent" if p.get("med") is None else str(p["med"])) for p in paths)
print(
    len(paths),
    sum(1 for p in paths if p.get("is_best")),
    ",".join(meds) or "-",
    "yes" if any(asn > 65535 for p in paths for asn in p.get("as_path", [])) else "no",
)
'
)"

if [[ "${paths:-0}" -eq 3 ]]; then
  pass "all three paths are read"
else
  fail "all three paths are read" "3" "${paths:-0}"
fi

if [[ "${best:-0}" -eq 1 ]]; then
  pass "exactly one path is the best one"
else
  fail "exactly one path is the best one" "1" "${best:-0}"
fi

# The heart of ADR-0006: a MED nobody set is absent, not zero.
if [[ "${meds:-}" == "0,100,absent" ]]; then
  pass "the MEDs are absent, 0 and 100 — absent is not zero"
else
  fail "the MEDs are absent, 0 and 100" "0,100,absent" "${meds:-none}"
fi

# VRP prints 32-bit ASNs in asdot (`1.0` is 65536) and they were dropped,
# turning a ten-hop path into a plausible four-hop one.
if [[ "${has_32bit:-no}" == "yes" ]]; then
  pass "the 32-bit AS numbers survive the path"
else
  fail "the 32-bit AS numbers survive the path" "an ASN above 65535" "none"
fi

# --- Ping -------------------------------------------------------------------
echo
echo "ping $TARGET_V4"
ping="$(query ping "$TARGET_V4")"

read -r sent received loss <<<"$(
  printf '%s' "$ping" | python3 -c '
import json, sys
try:
    r = json.load(sys.stdin).get("result", {})
except Exception:
    print("0 0 -")
    raise SystemExit
print(r.get("packets_sent", 0), r.get("packets_received", 0), r.get("packet_loss_percent", "-"))
'
)"

# A ping that answered every probe was reported as total loss, beside round
# trip times it had read correctly.
if [[ "${sent:-0}" -gt 0 && "${sent:-0}" == "${received:-x}" && "${loss:-x}" == "0.0" ]]; then
  pass "the ping is answered and counted ($received/$sent, $loss% loss)"
else
  fail "the ping is answered and counted" "sent == received, 0% loss" \
    "${received:-?}/${sent:-?}, ${loss:-?}% loss"
fi

# --- Traceroute -------------------------------------------------------------
echo
echo "traceroute $TARGET_V4"

# Twice, and the second one counts.
#
# The lab's silent hop is a rate limit, not a block — one ICMP error every two
# thousand seconds, which is how a real router behaves and why the wiki page
# says a silent hop is normal. After a long idle it has a token saved, so the
# first traceroute of the day gets an answer from it and the second does not.
# Checking the first would make this pass or fail on how long the lab has been
# sitting there.
query traceroute "$TARGET_V4" >/dev/null
trace="$(query traceroute "$TARGET_V4")"

read -r hops first_timings last_ip <<<"$(
  printf '%s' "$trace" | python3 -c '
import json, sys
try:
    hops = json.load(sys.stdin).get("result", {}).get("hops", [])
except Exception:
    print("0 - -")
    raise SystemExit
first = len(hops[0].get("rtt_ms", [])) if hops else -1
print(len(hops), first, (hops[-1].get("ip") if hops else None) or "-")
'
)"

if [[ "${hops:-0}" -ge 2 ]]; then
  pass "the trace reaches its destination ($hops hops, last $last_ip)"
else
  fail "the trace reaches its destination" "at least 2 hops" "${hops:-0}"
fi

# Two bugs meet here: a hop that timed out was given a round trip of 0.0 ms,
# and the reader gave up while the silent hop was silent and returned nothing.
if [[ "${first_timings:-1}" -eq 0 ]]; then
  pass "the silent hop keeps its number and carries no timing"
else
  fail "the silent hop keeps its number and carries no timing" \
    "no round trip time on hop 1" "${first_timings:-?} timings"
fi

# --- Result -----------------------------------------------------------------
echo
if [[ $failed -eq 0 ]]; then
  printf '\033[32m%d checks passed\033[0m against %s\n' "$passed" "$ROUTER"
  exit 0
fi

printf '\033[31m%d of %d checks failed\033[0m against %s\n' \
  "$failed" "$((passed + failed))" "$ROUTER"
exit 1
