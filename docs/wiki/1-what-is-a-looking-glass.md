# What is a looking glass?

## TL;DR

It is a window into someone else's router. An Internet provider publishes it so
that you — a customer, a peer, an engineer somewhere else — can ask their
network a question and get the router's own answer, without an account and
without asking a human. It only ever looks. Nothing you do here changes
anything on their network.

## The question it answers

Networks fail in a way that is hard to debug alone: **your side looks fine and
theirs looks fine, and traffic still does not arrive**. Each side only sees its
own half.

A looking glass hands you the other half. You ask their router "do you have a
route to my addresses, and what does it look like from there", and you read the
answer yourself.

Typical moments:

- You moved your service to a new address range and want to know whether the
  world can see it yet.
- Your traffic to one provider is slow and you want to see the path from their
  side.
- You are choosing an upstream and want to see how they reach your network.
- Someone told you your prefix is being announced by the wrong network and you
  want to check.

## What it cannot do

- **It cannot change anything.** Every command it runs is read-only, and the
  account it uses on the router cannot enter configuration mode. That is
  enforced on the router itself, not just in this software.
- **It cannot test your own network.** It shows what *their* router sees. If
  your provider's looking glass has no route to you, the problem might be
  yours, theirs, or someone's in between.
- **It is not monitoring.** It answers when you ask. Queries are rate limited,
  because a router's processor is a shared resource and this one belongs to
  someone else.

## What you can ask

| Query | The question |
|---|---|
| **Ping** | Can this router reach an address, and how long does it take? |
| **Traceroute** | Which routers does traffic pass through on the way there? |
| **BGP route** | What does this router know about this prefix — which paths, from whom, which is preferred? |
| **BGP summary** | Which of this network's sessions with other networks are up? |

The first two are about reachability right now. The last two are about routing:
what this network has been *told* about the destination, and by whom.

## A word about the answers

Two habits will save you time.

**Absent is not zero.** When a router does not report a value, this shows "not
reported" rather than a number. A metric of 0 and a metric nobody mentioned
mean different things, and a tool that prints 0 for both is lying to you.

**The raw output is always there.** Everything is parsed into tables and a
graph, but the exact text the router printed is at the bottom of every result.
If a table looks wrong, read the text — and if they disagree, that is a bug
worth reporting.

## Next

[Running your first query](2-your-first-query.md).
