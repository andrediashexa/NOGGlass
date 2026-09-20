# When something looks wrong

## TL;DR

Start by asking which of three things you are looking at: a limitation of this
software, a configuration on the instance you are using, or a real problem on
the network. This page sorts the common symptoms into those three piles.

## A result came back as plain text

**A parser gap, not a network problem.** That vendor has no parser for that
query yet, so you get exactly what the router printed. The answer is real; only
the table is missing.

Worth an issue with the raw output attached, redacted. That is how the parsers
get written.

## A column says "not reported"

**Not a problem at all.** The router did not print that value, and NOGGlass will
not invent one. An absent MED and a MED of zero are different statements, and
showing `0` for both would be a lie.

If you need the value, read the router output at the bottom of the page.

## The parsed view says it is partial

Some lines of the output were not understood. What was parsed is shown; the
rest is in the raw output. The same issue-with-raw-output applies — a partial
parse is usually one column in one platform version.

## The answer is truncated

The output hit the instance's size cap, which exists so one query cannot pull a
full table through. Ask a more specific question: a single prefix rather than a
regular expression, one AS rather than a wide match.

## "Too many queries from your address"

The per-visitor rate limit. Wait the number of seconds in `Retry-After`. If you
need sustained access, ask the operator rather than working around it — most
will help, and looking glasses are published in good faith.

If a whole office hits this together, you are sharing an address, or the
instance sits behind a proxy whose addresses the operator has not listed, and
everyone is counted as one visitor. That one is worth reporting to them.

## "Too many queries are running"

The instance or that router is at its concurrency cap. Temporary; try again in a
moment, or pick another router.

## "The router could not be reached"

The SSH session failed. The message is deliberately vague towards visitors — it
would otherwise describe the operator's management network — so the detail is in
the operator's log, not on your screen. Nothing you can do from outside except
tell them.

## A session shows a state I do not recognise

NOGGlass passes through any state word it does not know rather than guessing at
it. Some platforms print states that are not in the standard set. If you look
it up and it turns out to be common, an issue naming the platform and the word
is welcome.

## The global view says unavailable

The comparison could not run — a slow or unreachable RIPEstat, or an unreadable
answer. **It is not agreement.** The router's answer stands on its own, and the
comparison simply did not happen.

## The footer says "dev"

That binary was not built by the release pipeline, so it does not know its own
version. Common on a locally built container. Not a problem, but it means you
cannot tell the operator which version they are running.

## Still stuck

Open an issue with three things: what you asked, what came back, and what you
expected. If a parser is involved, the raw router output matters more than
everything else — with addresses and AS numbers redacted if they are yours to
protect.

## Next

[Adding a vendor](10-adding-a-vendor.md).
