# ADR-0011 — Authenticated collaborative multi-tenant platform with PeeringDB/RDAP verification and encrypted router vault

- **Status:** Proposed — deferred to the `0.3.0` milestone
- **Date:** 2026-09-19
- **Deciders:** André Dias, Marcelo Gondim

## TL;DR

> **Deferred, not rejected.** On 2026-09-20 the maintainers decided that
> `0.1.0` ships a publicly queryable Looking Glass with no account required, as
> stated in the README and ADR-0006. This document describes a different
> product layer — a federated diagnostic network between verified operators —
> and it MUST NOT be implemented before a public Looking Glass works end to
> end. Two consequences need answering before it is accepted: custody of other
> operators' router credentials is a legal and operational liability that does
> not disappear because the blobs are encrypted, and forbidding anonymous
> queries removes the audience a looking glass exists for.


Transforms the Looking Glass from a single-tenant tool into an authenticated, collaborative multi-tenant platform for verified Autonomous System (ASN) operators.
Anonymous public queries are prohibited; all users MUST register and verify administrative authority over their declared ASN via the PeeringDB API or IANA-federated RDAP, followed by dynamic single-use token confirmation by corporate email.
Authenticated operators MAY enroll routers with credentials encrypted at rest using ChaCha20-Poly1305 in an isolated Alpine MariaDB database.
In return, verified operators gain reciprocal access to run operational diagnostics (`ping`, `traceroute`, `bgp_route`, `bgp_summary`) across all participant routers, displayed publicly by company name and location without exposing ASNs or management credentials.

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT",
"RECOMMENDED", "MAY" and "OPTIONAL" in this document are to be interpreted as described in
[RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

## 5W2H

| Question | Answer |
|---|---|
| **What** | Collaborative multi-tenant Looking Glass restricted to authenticated ASN operators with encrypted credential vaulting. |
| **Why** | Prevents Internet-wide command abuse, eliminates configuration exposure, guarantees reciprocal diagnostic visibility, and proves operator legitimacy. |
| **Who** | Autonomous System operators, network engineers, backend authentication engine, and isolated MariaDB container. |
| **Where** | `crates/looking-glass-server`, `crates/looking-glass-auth`, `crates/looking-glass-db`, and `docker-compose.yml`. |
| **When** | Core foundational architecture for release milestone `0.2.0`. |
| **How** | Validate ASN ownership via PeeringDB/RDAP, enforce email challenge-response, encrypt credentials with ChaCha20-Poly1305, and store in MariaDB over Unix socket. |
| **How much** | Zero recurring license costs; minimal storage impact (~50 MB database) and ~5 ms crypto overhead per SSH session establishment. |

## Context

The initial architectural drafts (ADR-0001 through ADR-0007) conceived the Looking Glass as a standalone, single-tenant deployment reading router inventories from static configuration files (`routers.yml`). While effective for private testing, this model presents significant operational limitations in real-world telecommunications:

1. **Vulnerability to External Abuse:** Publicly exposed Looking Glasses without authentication are routinely targeted by automated scrapers, port scanners, and volumetric denial-of-service attempts that exhaust control-plane CPU cycles on edge routers.
2. **Configuration Rigidity:** Managing credentials, connection endpoints, and router inventories in flat files requires redeployment or container restarts whenever edge hardware is added or decommissioned.
3. **Information Asymmetry:** Regional Internet Service Providers (ISPs) often maintain separate looking glasses with disparate interfaces, forcing network engineers to navigate multiple disconnected tools during routing incidents.
4. **Lack of Identity and Attribution:** Without authenticated user sessions, suspicious or unauthorized diagnostic sweeps cannot be audited or attributed to a specific network peer.

To address these challenges, the platform adopts an authenticated, community-driven multi-tenant architecture. Entry is gated by cryptographic and institutional proof of ASN control, fostering a trusted network of operators who share diagnostic vantage points securely.

## Decision

### 1. Mandatory Operator Registration and Gated Access

The Looking Glass SHALL NOT accept anonymous diagnostic queries. Every interaction with router execution endpoints (`/api/query`) MUST require an active, authenticated operator session.

Registration requires:
- **Autonomous System Number (ASN):** The numerical identifier of the applicant's network (e.g. `20111`).
- **Legal and Trade Name:** Retrieved automatically from PeeringDB with optional operator display customization.
- **Operator Name:** Full name of the responsible network engineer.
- **Corporate Email Address:** Matching the registered domain or Point of Contact (POC) for that ASN.
- **Password:** Salted and hashed using Argon2id according to OWASP guidelines.

### 2. Dual-Layer ASN Authority Verification (PeeringDB + IANA RDAP)

Before dispatching an account activation link, the backend MUST verify that the applicant's email address legitimately belongs to the declared ASN:

```mermaid
flowchart TD
    reg[Applicant Submits Registration<br/>ASN + Corporate Email] --> pdb{Query PeeringDB API<br/>https://www.peeringdb.com/api/net?asn=X}
    pdb -->|Record Found| chk_pdb{Email Domain matches<br/>Website or POC email?}
    chk_pdb -->|Match Confirmed| gen_token[Generate Dynamic CSPRNG Token<br/>256-bit entropy, 2-hour TTL]
    chk_pdb -->|No Match| rdap_bootstrap[Query IANA RDAP Bootstrap<br/>https://data.iana.org/rdap/asn.json]
    pdb -->|Not Found| rdap_bootstrap
    rdap_bootstrap --> rir_lookup{Direct Query to Authoritative RIR<br/>ARIN / RIPE / LACNIC / APNIC / AFRINIC}
    rir_lookup -->|ASN Found| chk_rdap{Email matches vCard<br/>Admin / Tech / Abuse?}
    chk_rdap -->|Match Confirmed| gen_token
    chk_rdap -->|No Match| rej_email[Reject Registration:<br/>Email does not match authoritative records]
    rir_lookup -->|Invalid ASN| rej_asn[Reject Registration:<br/>Unallocated or invalid ASN]
    gen_token --> hash_token[Persist SHA-256 hash of token in DB]
    hash_token --> send_mail[Dispatch Confirmation Email with Dynamic Link]
    send_mail --> user_click[Operator clicks dynamic link]
    user_click --> activate_acc[Account Activated:<br/>Tenant status set to Active]
```

1. **Tier 1 (PeeringDB Primary):** The engine queries `https://www.peeringdb.com/api/net?asn={ASN}`. If the ASN exists, it extracts `name`, `website`, and official contact domains. If the registrant's email domain corresponds to the official organization domain, authority is provisionally accepted.
2. **Tier 2 (IANA-Federated RDAP Authoritative Fallback):** If PeeringDB data is absent, incomplete, or unconfirmed, the engine consults the local cache of the IANA RDAP bootstrap allocations (RFC 9224). The engine queries the authoritative Regional Internet Registry (RIR) directly (e.g. `rdap.registro.br`, `rdap.arin.net`, `rdap.db.ripe.net`, `rdap.apnic.net`, `rdap.afrinic.net`). The applicant's email domain MUST match an authorized contact entity (`administrative`, `technical`, `noc`, or `abuse`).
3. **Cryptographic Email Activation Challenge:** Upon successful authority validation, the engine generates a 256-bit cryptographically secure pseudorandom token (`rand::rngs::OsRng`). The token's SHA-256 hash is recorded in MariaDB with a strict 2-hour expiration and single-use invalidation. The unhashed token is dispatched via email. The account remains disabled until the dynamic URL is visited.

### 3. Zero-Knowledge Credential Vaulting (At-Rest Encryption)

Router management credentials (passwords, private SSH keys, and internal management IP addresses) MUST NOT be stored in plaintext.

```mermaid
flowchart LR
    input[Operator enters SSH Credentials in UI] --> https[TLS 1.3 Transport]
    https --> server[Looking Glass Backend Engine]
    env[Master Key<br/>LOOKING_GLASS_MASTER_KEY] --> kdf[HKDF-SHA256 Derivation]
    kdf --> cipher[ChaCha20-Poly1305 AEAD]
    server --> cipher
    cipher -->|Encrypted Ciphertext + Nonce| mariadb[(Isolated Alpine MariaDB<br/>Zero External Ports)]
    mariadb -.->|Decrypted only in memory| session[Ephemeral SSH Session to Edge Router]
```

- **Encryption Algorithm:** Symmetric authenticated encryption using ChaCha20-Poly1305 (RFC 8439) with a unique 96-bit random nonce per record.
- **Key Management:** The 256-bit master secret (`LOOKING_GLASS_MASTER_KEY`) MUST be injected exclusively via runtime environment variables and MUST NOT exist in database dumps, configuration files, or version control.
- **Memory Lifetime:** Decrypted credentials SHALL exist only ephemerally in RAM during the lifecycle of the SSH session and MUST be scrubbed from memory immediately upon connection completion.
- **Frontend Masking:** Administrative endpoints listing registered routers SHALL NEVER transmit decrypted passwords or private keys back to the client; fields MUST be masked with redacted placeholders.

### 4. Reciprocal Collaborative Catalog (Company and Location Vantage Points)

Once logged in, operators gain access to a collaborative diagnostic selector:

```mermaid
flowchart TD
    auth_user[Authenticated Operator] --> selector[Interactive Router Selector]
    selector --> list[Company Name + Location Display]
    subgraph catalog[Visible Data to All Members]
        item1[Provider Alpha - São Paulo / SP - Equinix SP4]
        item2[Provider Beta - Fortaleza / CE - Hostsite]
        item3[Carrier Gamma - Frankfurt / DE - Interxion]
    end
    list --> catalog
    catalog -->|Selects Target Box| query[Dispatches /api/query with Router UUID]
    query --> exec[Backend resolves internal credentials via Vault]
    exec --> router[Target Edge Router executes command]
    router --> result[Normalized Result + Canvas 2D AS-PATH rendered to Operator]
```

- **Anonymized Organization Presentation:** The selector displays the organization's business name and physical router location (city, state/province, datacenter). The ASN number is deliberately omitted from the primary selection list to mitigate scraping and targeting.
- **Opaque UUID Addressing:** The frontend references routers solely by immutable UUIDs (e.g. `router_id: "550e8400-e29b-41d4-a716-446655440000"`). Internal management IPs, ports, and usernames are completely concealed from the browser.
- **Reciprocity Policy:** Verified tenants who contribute at least one operational router to the platform obtain unrestricted query access to all community vantage points. Tenants who deactivate all routers are downgraded to private-only diagnostic mode.

### 5. Infrastructure Isolation: Alpine MariaDB via Unix Domain Socket

To prevent credential exfiltration through network vulnerabilities:
- The database runs in a dedicated, minimal `mariadb:lts-alpine` container.
- The MariaDB container MUST NOT expose port 3306 on the host network (`ports:` directive prohibited).
- Communication between the Looking Glass backend and MariaDB SHALL occur through a shared Docker volume containing a Unix domain socket (`/run/mysqld/mysqld.sock`) or an internal Docker bridge network unreachable from external interfaces.

## Consequences

### Positive

- **Elimination of Abuse:** Requiring authenticated sessions for all queries prevents automated scanning and control-plane exhaustion on member routers.
- **High-Fidelity Trust Fabric:** Validation against PeeringDB and IANA RDAP ensures only verified network operators participate in the community.
- **Robust Defense-in-Depth:** Compromise of the MariaDB database reveals only encrypted blobs; credentials cannot be decrypted without the host master key.
- **Collective Intelligence:** Operators gain real-time external vantage points across diverse carriers and regions without negotiating separate peering agreements.

### Negative

- **Onboarding Friction:** Operators must complete email verification and have up-to-date PeeringDB or RDAP records to use the platform.
- **Key Custody Responsibility:** Loss of the `LOOKING_GLASS_MASTER_KEY` permanently renders all vaulted router credentials unrecoverable, requiring re-enrollment.
- **Outbound Email Dependency:** Requires reliable transactional SMTP or API mailer configuration for activation challenges.
