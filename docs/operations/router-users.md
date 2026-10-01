# Read-only router users

## TL;DR

Create a dedicated account per router, with the smallest role that can run the
four commands NOGGlass sends, and nothing else. The per-vendor recipes below are
a starting point, not a policy: check them against your own hardening standard
before pasting. Whatever you configure, verify it from a shell first — if the
account can enter configuration mode, it is too strong.

## One thing that is not about BGP

On a router whose operating system is Linux — BIRD, FRR, VyOS and anything else
where the looking glass logs into a shell — **traceroute opens a raw socket**,
and an unprivileged account may not. The query comes back with no hops and the
router's own message explains why:

```
traceroute: socket(AF_INET,3,1): Operation not permitted
```

Ping is usually fine, because the kernel allows it for a group range without
any privilege. Traceroute needs `CAP_NET_RAW` on the binary:

```bash
setcap cap_net_raw+ep /usr/bin/traceroute
```

A vendor CLI does not have this problem: there the privilege model is the
vendor's, and a read-only account that can run `traceroute` runs it.

## 5W2H

| Question | Answer |
|---|---|
| **What** | The router-side account NOGGlass authenticates with, per vendor. |
| **Why** | A public endpoint reaches these routers; the account it uses is the blast radius. |
| **Who** | The network operator, before pointing NOGGlass at anything. |
| **Where** | On each router in the inventory. |
| **When** | Before the first query against a real device. |
| **How** | A dedicated user, a read-only role, and a check that it cannot configure. |
| **How much** | Minutes per router; no licence and no extra hardware. |

## Rules

1. Each router SHOULD have its own account and its own password or key. One
   shared account makes an incident impossible to attribute.
2. The account MUST NOT be able to enter configuration mode, write files, or
   reload. NOGGlass never needs any of those and never asks for them.
3. The account SHOULD be restricted by source address to the NOGGlass host.
4. Where the platform supports it, key authentication SHOULD be preferred over a
   password.
5. The commands NOGGlass will actually send are printed by
   `GET /api/catalogue/{vendor}`. Read them before granting anything.

```mermaid
flowchart LR
    lg[NOGGlass host] -->|SSH from a known address| acl{Router ACL}
    acl -->|permitted| user[Dedicated read-only user]
    acl -->|anything else| drop[Dropped]
    user --> cmds["Only the catalogue commands:<br/>ping · traceroute · show bgp"]
    user -.->|MUST NOT| config[Configuration mode]
```

## Huawei VRP

The reference vendor for `0.1.0`. Level 1 is the visit level: it can run
display and diagnostic commands and cannot configure.

```text
aaa
 local-user nogglass password irreversible-cipher <password>
 local-user nogglass service-type ssh
 local-user nogglass privilege level 1
 local-user nogglass ftp-directory  # not needed; leave unset
quit

ssh user nogglass authentication-type password
ssh user nogglass service-type stelnet
stelnet server enable
```

Restrict where it may come from:

```text
acl number 2000
 rule 5 permit source <nogglass-host> 0
 rule 10 deny
quit
ssh server acl 2000
```

Verify: log in as the account and confirm that `system-view` is refused, and
that `display bgp routing-table 198.51.100.0 255.255.255.0` works.

## Cisco IOS-XE

```text
username nogglass privilege 1 secret <password>
ip ssh version 2

! Privilege 1 already excludes configuration. Allow the diagnostics explicitly
! if your baseline restricts them.
privilege exec level 1 show bgp
privilege exec level 1 ping
privilege exec level 1 traceroute

line vty 0 4
 access-class NOGGLASS in
 transport input ssh
```

## Cisco IOS-XR

```text
username nogglass
 group read-only-tg
 secret <password>
!
```

`read-only-tg` is the built-in read-only task group. Confirm with
`show user tasks` while logged in as the account.

## Juniper Junos

```text
set system login class nogglass-ro permissions [ view routing network ]
set system login user nogglass class nogglass-ro
set system login user nogglass authentication ssh-ed25519 "<public key>"
set system services ssh protocol-version v2
```

A dedicated class with permissions `[ view routing network ]` (or the built-in
`operator` class) covers `show` commands, `ping` and `traceroute` from
operational mode while strictly excluding configuration mode. The default
`read-only` class lacks the `network` permission required for ping and traceroute.

## Nokia SR OS

```text
/configure system security profile "read-only"
    entry 20
        match "environment"
        action permit
    exit

/configure system security user "nogglass"
    password <password>
    access console
    console member "read-only"
```

The `environment` CLI permission is REQUIRED for NOGGlass to execute `environment no more` and disable output pagination non-interactively. Without it, queries with more than 24 lines trigger interactive pagination (`Press any key to continue`) and time out.

## MikroTik RouterOS

RouterOS groups are explicit, so grant the minimum and nothing else:

```text
/user group add name=nogglass policy=ssh,read,test,!write,!policy,!ftp,!reboot,!password,!sniff,!sensitive,!romon
/user add name=nogglass group=nogglass password=<password> address=<nogglass-host>
```

`test` is what `/ping` and `/tool traceroute` need; `!sensitive` keeps stored
secrets out of the account's view.

## Datacom DmOS

```text
configure
 aaa authentication user nogglass password <password> group operator
commit
```

`operator` is the read-only group on DmOS. Verify that `configure` is refused.

## BIRD

BIRD is reached through its control socket or through a shell account. The
account NOGGlass uses SHOULD have read access to the socket and nothing else:

```bash
usermod -aG bird nogglass        # read access to /run/bird/bird.ctl
```

`birdc` is read-only for `show` commands; configuration changes need a separate
privilege, so do not grant `sudo`.

## Arista EOS

```text
username nogglass privilege 1 role network-operator secret <password>
management ssh
   idle-timeout 5
```

## After configuring

From the NOGGlass host, as the account:

```bash
ssh nogglass@192.0.2.10
# Then, on the router, confirm two things:
#   1. the diagnostic commands work
#   2. configuration mode is refused
```

Then let NOGGlass try it:

```bash
curl -s -X POST localhost:8080/api/query \
  -H 'content-type: application/json' \
  -d '{"router":"edge-01","type":"bgp_route","target":"198.51.100.0/24"}'
```

If the answer comes back as raw text instead of a table, the account works and
the parser does not cover that output yet — report it with the raw output
attached, and redact anything you would not publish.
