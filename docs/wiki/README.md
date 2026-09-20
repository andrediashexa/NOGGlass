# Using NOGGlass

## TL;DR

Ten pages: six for someone reading an answer, four for someone running the
instance. Start at the top if you were handed a link; skip to page 7 if you are
the operator. The [outline](../wiki-outline.md) says what each page is meant to
achieve and how to tell when it is finished.

## For a visitor

| Page | The question |
|---|---|
| [1. What is a looking glass?](1-what-is-a-looking-glass.md) | What is this, and what can it not do? |
| [2. Running your first query](2-your-first-query.md) | Which of the four questions do I want? |
| [3. Reading a BGP result](3-reading-a-bgp-result.md) | What do the columns mean, and why is the best path not the shortest? |
| [4. RPKI: four states](4-rpki-four-states.md) | Is this announcement authorised, and who says so? |
| [5. Router versus Internet](5-router-versus-internet.md) | My prefix is missing — where? |
| [6. A traceroute that stops](6-traceroute-that-stops.md) | Why are there stars, and are they the problem? |

## For an operator

| Page | The question |
|---|---|
| [7. Connecting a router](7-connecting-a-router.md) | How do I point this at something real, and prove it works? |
| [8. Publishing it](8-publishing-it.md) | What can a stranger make my routers do? |
| [9. When something looks wrong](9-when-something-looks-wrong.md) | Is this the software, the instance, or the network? |
| [10. Adding a vendor](10-adding-a-vendor.md) | How do I teach it a platform it does not know? |

## Publishing this to the GitHub wiki

These pages are the source. `scripts/publish-wiki.sh` generates the GitHub wiki
from them — renaming files to the wiki convention, rewriting links and carrying
the screenshots across — so the wiki is a mirror and an edit made there is
replaced on the next run. That is deliberate: a page edited in the wiki skips
the documentation linter and the review rules.

It does not run yet. GitHub allows wikis in a private repository only on a paid
plan, and the API accepts the setting while silently leaving it off. Making the
repository public enables it, and also enables the branch protection this
project has been doing by discipline.

```bash
scripts/publish-wiki.sh --dry-run   # see what would be published
scripts/publish-wiki.sh             # publish, once the wiki exists
```

## Elsewhere

Installing and configuring is in [operations](../operations/deployment.md); the
reasoning behind the product is in the [decisions](../adr/). Neither is repeated
here — a fact kept in two places becomes wrong in one of them.
