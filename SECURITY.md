# Security Policy

## TL;DR

NOGGlass connects to production routing equipment, making security our highest priority. Vulnerability reports MUST be disclosed privately, never via public issues or discussion forums. Contact the core maintainers via GitHub Security Advisories or direct secure email.

---

## Reporting Security Issues

Please do not open public GitHub issues or public pull requests for security vulnerabilities.

Instead, report vulnerabilities privately through one of the following channels:
1. **GitHub Private Vulnerability Reporting:**
   [Submit a Security Advisory](https://github.com/andrediashexa/nogglass/security/advisories/new)
2. **Direct Maintainer Email:**
   - Marcelo Gondim da Cunha: [gondim@ispfocus.net.br](mailto:gondim@ispfocus.net.br) / [gondim@gmail.com](mailto:gondim@gmail.com)
   - André Dias: [andreluizroddias@gmail.com](mailto:andreluizroddias@gmail.com)

---

## What to Include in Your Report

To help us triage and resolve the issue quickly, please include:
- Affected component or crate (`looking-glass-core`, `nogglass-server`).
- Target router vendor/driver if vendor-specific (Huawei, Juniper, Cisco, MikroTik, etc.).
- Clear reproduction steps or minimal Proof-of-Concept (PoC).
- Detailed impact assessment (denial of service, control plane exhaustion, credential exposure, bypass of rate limits).
- Suggested remediation or patch (optional).

---

## Response and Coordination Process

1. **Acknowledgement:** We will acknowledge receipt of your vulnerability report within 48 to 72 hours.
2. **Assessment:** The core maintainers will verify the vulnerability and assess its operational impact on production networks.
3. **Remediation:** A patch will be engineered, reviewed, and verified against simulated router targets.
4. **Responsible Disclosure:** We adhere to a 90-day responsible disclosure window. Fixes will be published in a patch release before public disclosure.

---

## Testing Policy and Operator Boundaries

- **Permission Required:** You MUST NOT perform penetration testing, denial-of-service simulations, or automated vulnerability scans against any third-party Looking Glass instance without explicit, written authorization from the system operator.
- **Dedicated Accounts:** Operators deploying NOGGlass MUST configure dedicated, unprivileged, read-only accounts on all managed routers, avoiding administrative or write-access credentials.
- **Defense-in-Depth:** In production environments, keep the management interface off the public Internet, pin SSH host keys (`host_key`), and enforce CAPTCHA on public-facing deployments.
