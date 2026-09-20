# Reading a BGP result

## TL;DR

A BGP lookup answers "who told this router how to reach that prefix, and which
of those offers did it take". The graph shows the offers; the table shows why
one won. The single most misread thing on the page is **"not reported"**, which
means the router said nothing about a value — it does not mean zero.

## What you are looking at

![A BGP route result](../design/screens/02-rota-bgp.png)

The router usually knows **several ways** to reach a prefix, learned from
different neighbours. It picks one to actually use. The graph draws the choices:
the router on the left, the network that originated the prefix on the right, and
the networks in between.

- **Solid line** — the path the router chose. Traffic goes this way.
- **Dashed line** — a path it knows about and did not choose. It is the backup:
  if the chosen one disappears, traffic moves here, usually in seconds.

The table below lists the same paths with the numbers behind the choice.

## The columns

**Prefix.** The block of addresses this route covers. A router can hold a
route for `198.51.100.0/24` and a more specific `198.51.100.0/25`; the more
specific one wins for addresses inside it, always, regardless of everything
below.

**Next hop.** The neighbouring router the traffic is handed to. It is the first
step, not the destination.

**AS path.** The networks the announcement travelled through, nearest first.
`65100 65500` means "my neighbour AS65100 told me, and they heard it from
AS65500, who owns it". The last number is the **origin**: the network that
claims these addresses.

Shorter is often preferred, but **not always** — read on.

**Local preference.** The operator's own policy, and it beats everything except
a more specific prefix. A network might set 200 on routes from a paid transit
they trust and 100 on a cheaper one; the 200 wins even if its AS path is longer.
When you see a longer path chosen, this is usually why. It is local: your
neighbour's local preference means nothing to your router.

**MED.** A hint *from* the neighbouring network about which of their several
links they prefer you to use. Lower is better. It only compares paths from the
same neighbour, and many networks ignore it.

**Weight.** Same idea as local preference but even more local: it never leaves
the router it is set on. Cisco-specific in origin.

**Origin.** How the prefix entered BGP in the first place: `IGP` (announced
deliberately), `EGP` (historical, you will not see it), `incomplete` (usually
redistributed from another protocol). Rarely decides anything.

**Communities.** Tags the operator attaches to a route, like `65001:100`. They
encode that operator's policy — "learned from a customer", "do not announce to
Europe", "this is a blackhole" — and **only that operator knows what theirs
mean**. NOGGlass shows them exactly as printed rather than guessing, except for
the handful that are standard everywhere, like `no-export` and `blackhole`.

## Why the best path is not the shortest one

The router walks a list of tie-breakers, in order, and stops at the first that
decides:

1. **Higher weight** — local to this router
2. **Higher local preference** — the operator's policy, and where most decisions
   are actually made
3. Locally originated routes
4. **Shorter AS path** — the one everybody knows about, and it is fourth
5. Lower origin type, then lower MED, then external over internal, then closer
   internal cost, then an arbitrary but stable tie-break

So: if you expected the short path to win and it did not, look at local
preference before anything else. If local preference is equal and the paths are
the same length, look at MED. The order is the whole story.

## "not reported" is not zero

Some values are simply absent from what a router printed. NOGGlass shows those
in muted italics as **not reported**, and never as `0`.

This matters because a MED of `0` is a real statement — the neighbour is
expressing a preference — while an absent MED means nobody said anything. Tools
that print `0` for both have caused real arguments between operators. If you
need the value, the router's raw output is at the bottom of the page.

## What to check when something looks wrong

| What you see | What it usually means |
|---|---|
| No path at all | This router has no route. The announcement may not have reached them, or they filter it |
| Only one path, no backup | Single-homed towards that destination, or filters removed the others |
| AS path longer than expected | Local preference chose it; look at that column |
| An origin AS you do not recognise | Read [RPKI: four states](4-rpki-four-states.md) — this might matter |
| Nothing in communities | This operator does not tag, or strips tags on receipt |

## Next

[RPKI: four states, and who said so](4-rpki-four-states.md).
