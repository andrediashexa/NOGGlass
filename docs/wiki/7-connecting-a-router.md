# Connecting your first real router

## TL;DR

Create a read-only account on the router, add an entry to `nogglass.toml`
naming an environment variable for its password, restart, and compare one
answer against the CLI before you trust any of them. The comparison is the
step people skip and the only one that proves anything.

## Before you touch the configuration

**Create the account first**, and verify it by hand. The recipe per vendor is in
[read-only router users](../operations/router-users.md); the rules are the same
everywhere:

- One account per router, not one shared with humans, so its activity is
  distinguishable in your logs.
- The smallest read-only role the platform offers.
- Restricted to the NOGGlass host by source address.

Then log in as that account and confirm two things: the diagnostic commands
work, and **configuration mode is refused**. If the account can configure, stop
and fix that before going further.

## See what will be sent

Before pointing anything at production, ask NOGGlass what it would run:

```bash
curl -s localhost:8080/api/catalogue/huawei_vrp
```

```json
{
  "vendor": "huawei_vrp",
  "disable_paging": "screen-length 0 temporary",
  "ping": "ping -c {count} {target}",
  "bgp_route": "display bgp routing-table {network} {netmask}",
  "bgp_summary": "display bgp peer"
}
```

Those are the only commands that vendor will ever receive. The placeholders are
filled with typed values — an address, a prefix, a number — never with text a
visitor typed. Read them, and if one is wrong for your platform version, open
an issue before connecting anything.

## Add the router

```toml
[[router]]
id = "edge-01"                 # appears in URLs; do not change it later
name = "Edge 01"               # what visitors see
vendor = "huawei_vrp"
host = "192.0.2.10"            # management address, never shown to visitors
username = "nogglass"
location = "Sao Paulo"         # groups the selector
credentials = { password_env = "NOGGLASS_EDGE01_PASSWORD" }
queries = ["ping", "traceroute", "bgp_route", "bgp_summary"]
```

The password is **named** here and **read from the environment**, so this file
stays free of secrets and can live in version control:

```bash
NOGGLASS_EDGE01_PASSWORD=...     # your secret store, systemd unit, or .env at mode 600
```

`queries` is a whitelist. Leave it out to offer everything the vendor supports,
or narrow it — BGP lookups on the border, ping and traceroute on aggregation.

Restart. NOGGlass validates the file at startup and refuses to run if it does
not understand it, naming exactly what it refused, including a password
variable that is not set.

## Prove it

Run one query and compare it with the CLI, by hand, once:

```bash
curl -s -X POST localhost:8080/api/query \
  -H 'content-type: application/json' \
  -d '{"router":"edge-01","type":"bgp_route","target":"198.51.100.0/24"}'
```

Then run the same command on the router and read both. Check the prefix, the
next hop, the AS path, and especially the **numeric columns** — local
preference, MED, weight. Column-position mistakes are the classic parser bug,
and they are silent: the numbers look plausible, they are just in the wrong
place.

This matters more than usual here. **Every parser in this project was written
from documented output, not from a real device.** You may be the first person to
point it at your platform and version. If a column is wrong, open an issue with
the raw output, redacted — that is the single most useful contribution anyone
can make right now.

## When the answer comes back as raw text

The vendor's parser does not cover that query yet. You still get the router's
output, and the interface says it could not be read. Worth an issue with the
raw text attached.

## Next

[Publishing it without regretting it](8-publishing-it.md).
