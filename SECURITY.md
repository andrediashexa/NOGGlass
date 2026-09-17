# Security Policy

## TL;DR

A Looking Glass is a public endpoint that reaches production routers, so
vulnerability reports are welcome and MUST be sent privately, not as a public
issue. Report through GitHub Security Advisories on this repository. Expect an
acknowledgement within 5 business days.

## Reporting a vulnerability

1. Open a private report at
   `https://github.com/andrediashexa/looking-glass/security/advisories/new`.
2. Include the affected version, a description of the impact and the minimal
   steps to reproduce it.
3. Do NOT open a public issue, and do NOT test against a third party's Looking
   Glass instance without written authorisation from its operator.

Reporters SHOULD allow a 90-day disclosure window. Fixes are released as a patch
version and described in the changelog once available.

## Supported versions

While the project is in `0.x`, only the latest released version receives
security fixes.

## Operator responsibilities

Self-hosted deployments are the operator's responsibility. Operators SHOULD:

- Give the Looking Glass a dedicated, read-only router user per vendor, as
  documented in the deployment guide.
- Keep the backend's management interface unreachable from the Internet.
- Enable rate limiting and, on a public instance, the CAPTCHA integration.
- Keep the stack updated: security fixes ship as new container images.
