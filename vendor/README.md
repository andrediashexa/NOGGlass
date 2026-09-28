# Vendored dependencies

## netgauze-bgp-speaker (temporary, one-line fix)

A verbatim copy of `netgauze-bgp-speaker` 0.13.0 with a single upstream bug
fixed, pulled in via `[patch.crates-io]` in the workspace `Cargo.toml`.

**Why:** 0.13.0's post-decode mandatory-attribute check in `src/connection.rs`
breaks out of its loop once ORIGIN and AS_PATH are seen, so with the standard
attribute order (ORIGIN, AS_PATH, NEXT_HOP, …) it never observes NEXT_HOP,
wrongly raises `MissingWellKnownAttribute(NEXT_HOP)`, and resets the session —
breaking NOGGlass's BGP session (#171) against essentially every real peer. The
fix is removing that early `break`; the change is the only difference from the
published crate.

**This is temporary.** Remove this directory and the `[patch.crates-io]` entry
once the fix is released upstream, and depend on the published crate again
(the pinned-dependencies rule wants exactly that). The fix has been reported /
is captured in `lab/localbgp/README.md`. Upstream is Apache-2.0 (see LICENSE,
NOTICE).
