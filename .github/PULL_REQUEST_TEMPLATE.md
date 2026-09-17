<!--
  Title MUST follow Conventional Commits: <type>(<scope>): <description>
  Branch MUST be <type>/<issue-number>-<slug>
  Both are checked by CI.
-->

## What changed

<!-- One paragraph, from the user's point of view. -->

Closes #

## How it was verified

<!-- Commands run, vendors tested against, screenshots for UI changes. -->

## Checklist

- [ ] Documentation updated in this pull request (`scripts/check-docs.sh` passes)
- [ ] User-facing strings exist in pt-BR, en and es (`scripts/check-i18n.sh` passes)
- [ ] No user input reaches a router command line
- [ ] New router commands are read-only, with a timeout and an output cap
- [ ] No secret committed
