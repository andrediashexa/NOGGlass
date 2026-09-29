# Changelog

## [1.1.0](https://github.com/andrediashexa/NOGGlass/compare/v1.0.0...v1.1.0) (2026-09-29)


### Features

* **core:** support per-router source ipv4 and ipv6 for ping and traceroute ([7a38214](https://github.com/andrediashexa/NOGGlass/commit/7a3821489a1b3954fb7ead03834084d2dedd6101))
* **web:** display route age in bgp paths table ([9ffa894](https://github.com/andrediashexa/NOGGlass/commit/9ffa8943225b38ad06faf148d375fc665a90f76a))


### Documentation

* add the community announcement text ([#230](https://github.com/andrediashexa/NOGGlass/issues/230)) ([1ec11c6](https://github.com/andrediashexa/NOGGlass/commit/1ec11c617fcf2f26a64cfa3004fa7e5c08502dfd)), closes [#229](https://github.com/andrediashexa/NOGGlass/issues/229)
* credit Marcelo Gondim as the project's originator ([#227](https://github.com/andrediashexa/NOGGlass/issues/227)) ([cf2d45a](https://github.com/andrediashexa/NOGGlass/commit/cf2d45a704d2a0dc81d9dee5702f9230bb6747a5)), closes [#226](https://github.com/andrediashexa/NOGGlass/issues/226)
* document route age, custom source ip and ssh port in changelog for v1.1.0 ([9806d44](https://github.com/andrediashexa/NOGGlass/commit/9806d4495f9731468cd2b16f781c5010abaee44f))
* **infra:** document and test custom ssh port per router with default 22 ([01b21b0](https://github.com/andrediashexa/NOGGlass/commit/01b21b057deca0c5aff3b8b185934f64b299fbd0))
* update the community announcement for the 1.0.0 launch ([#233](https://github.com/andrediashexa/NOGGlass/issues/233)) ([8997594](https://github.com/andrediashexa/NOGGlass/commit/899759445534fd398c72a32b6aa47e5f185a6557)), closes [#232](https://github.com/andrediashexa/NOGGlass/issues/232)


### Style

* format inventory and executor according to cargo fmt ([7898168](https://github.com/andrediashexa/NOGGlass/commit/789816893e615d5dd72ae97a16692f40cef6deec))


### Chores

* bump workspace version to 1.1.0 ([c09a90d](https://github.com/andrediashexa/NOGGlass/commit/c09a90dc8522feb3192e2dde1795484efb9e6527))

## [1.1.0](https://github.com/andrediashexa/NOGGlass/compare/v1.0.0...v1.1.0) (2026-09-29)

### Features

* **web:** display BGP route installation age/duration in BGP paths table across all supported vendors
* **core:** support router-specific source IPv4 (`source_v4`) and IPv6 (`source_v6`) for diagnostic ping and traceroute queries
* **inventory:** support custom SSH port (`port`, defaults to 22) per edge router in `routers.conf`

### Documentation

* **inventory:** document `port`, `source_v4`, and `source_v6` in `routers.example.conf`, `INSTALL.md`, and deployment guides

## [1.0.0](https://github.com/andrediashexa/NOGGlass/compare/v0.9.3...v1.0.0) (2026-09-29)

### Features

* **drivers:** complete Juniper JunOS end-to-end integration with native CLI reconstruction for routes, BGP summary and AS-Path queries
* **config:** support modular configuration files (`nogglass.conf`, `routers.conf`, `ui.conf`) with automatic companion discovery
* **ui:** deliver high-contrast NOC Dark and Light themes with dedicated brand assets and wallpapers
* **topology:** render AS-Path graph via Bellman-Ford DAG relaxation with theme-adaptive styling and best-path priority
* **packaging:** publish static standalone Linux amd64 binaries (Zig & Musl) for native bare-metal deployment with Systemd

### Bug Fixes

* **compose:** use official GHCR image namespaces and isolate documentation service under profile
* **server:** provide dynamic fallback from `nogglass.conf` to `nogglass.toml` in container environments
* **security:** enforce strict SSH host key verification (Anti-MitM policy) with explicit transport diagnostics

### Chores

* **infra:** eliminate obsolete `nogglass.example.toml` in favor of official modular `.conf` templates
* **docs:** publish sanitized interface showcase screens adhering strictly to RFC 1918 and RFC 6996

## [0.9.3](https://github.com/andrediashexa/NOGGlass/compare/v0.9.2...v0.9.3) (2026-09-29)


### Chores

* **infra:** eliminate obsolete nogglass.example.toml in favor of modular conf templates ([e59a5e1](https://github.com/andrediashexa/NOGGlass/commit/e59a5e1b71bf96c9f27a88de2e7fd20187045392))

## [0.9.2](https://github.com/andrediashexa/NOGGlass/compare/v0.9.1...v0.9.2) (2026-09-29)


### Chores

* restore nogglass.example.toml for backward-compatible release packaging ([76e87e7](https://github.com/andrediashexa/NOGGlass/commit/76e87e79d27fee63e4132d8b6623e09fdf82c288))

## [0.9.1](https://github.com/andrediashexa/NOGGlass/compare/v0.9.0...v0.9.1) (2026-09-29)


### Bug Fixes

* **compose:** use full ghcr image name and isolate docs service under profile ([db06fb3](https://github.com/andrediashexa/NOGGlass/commit/db06fb3c65013d9972eff62809f188101f6d3f85))
* **server:** fallback to legacy nogglass.toml when nogglass.conf is missing in container ([6dc275a](https://github.com/andrediashexa/NOGGlass/commit/6dc275ae4237a926b1134a176e49c8bb535662d3))


### Documentation

* **vendor:** mark Juniper Junos as verified end to end ([7e3f6e3](https://github.com/andrediashexa/NOGGlass/commit/7e3f6e3559ab2216f544a3e83aab241c97f1abab))

## [0.9.0](https://github.com/andrediashexa/NOGGlass/compare/v0.8.1...v0.9.0) (2026-09-29)


### Features

* **ui,drivers:** add theme assets, modular config, juniper cli formatting, and sanitized showcase screens ([bd3be50](https://github.com/andrediashexa/NOGGlass/commit/bd3be505cb866345ecc31321227efa2ddde9c23e))


### Documentation

* record the federation 1.0-scope decision (recommend post-1.0) ([#217](https://github.com/andrediashexa/NOGGlass/issues/217)) ([e93219c](https://github.com/andrediashexa/NOGGlass/commit/e93219caf2daa969bc5993707968533d1ed34d3b)), closes [#78](https://github.com/andrediashexa/NOGGlass/issues/78)


### Tests

* **api:** cover the SSE streaming query endpoint ([#222](https://github.com/andrediashexa/NOGGlass/issues/222)) ([5ce8388](https://github.com/andrediashexa/NOGGlass/commit/5ce83883fd58dc487aedce4698d37a5c1812b8f8)), closes [#221](https://github.com/andrediashexa/NOGGlass/issues/221)
* **lab:** call bgp_summary with no target in verify.sh ([#219](https://github.com/andrediashexa/NOGGlass/issues/219)) ([bb5bd36](https://github.com/andrediashexa/NOGGlass/commit/bb5bd3695c4fffe6568c9859f38b355a4bb724e8)), closes [#218](https://github.com/andrediashexa/NOGGlass/issues/218)


### Style

* format code according to rustfmt guidelines ([5f9db1a](https://github.com/andrediashexa/NOGGlass/commit/5f9db1a2480659fc12596293149b46642948d8a6))

## [0.8.1](https://github.com/andrediashexa/NOGGlass/compare/v0.8.0...v0.8.1) (2026-09-28)


### Bug Fixes

* **api:** let bgp_summary run without a target ([#212](https://github.com/andrediashexa/NOGGlass/issues/212)) ([822db29](https://github.com/andrediashexa/NOGGlass/commit/822db2907f9f14537c6c03e3ebbde82ed09c7662)), closes [#211](https://github.com/andrediashexa/NOGGlass/issues/211)


### Documentation

* defer the BGP session and BMP engine to post-1.0 ([#216](https://github.com/andrediashexa/NOGGlass/issues/216)) ([5ab6b66](https://github.com/andrediashexa/NOGGlass/commit/5ab6b66dba604a50b70fe7cb6ef6ef30e588b7fd)), closes [#214](https://github.com/andrediashexa/NOGGlass/issues/214)

## [0.8.0](https://github.com/andrediashexa/NOGGlass/compare/v0.7.0...v0.8.0) (2026-09-28)


### ⚠ BREAKING CHANGES

* **bgp,bmp:** remove the embedded BGP session and BMP station ([#195](https://github.com/andrediashexa/NOGGlass/issues/195))

### Features

* **bgp:** answer bgp_aspath from the local RIB (BGP session and BMP) ([#189](https://github.com/andrediashexa/NOGGlass/issues/189)) ([7900308](https://github.com/andrediashexa/NOGGlass/commit/7900308356043893b005190f5d4190f951e5cd31))
* **bgp:** answer bgp_summary for NOGGlass's own BGP session ([#187](https://github.com/andrediashexa/NOGGlass/issues/187)) ([2139b41](https://github.com/andrediashexa/NOGGlass/commit/2139b410a66fe8d4632f07c257bd5067fd8e2a8d))
* **bgp:** answer local-RIB queries by longest-prefix match ([#183](https://github.com/andrediashexa/NOGGlass/issues/183)) ([57a56af](https://github.com/andrediashexa/NOGGlass/commit/57a56af4a828d312ab4c37ae87773ee8e9ac2dd1))
* **bgp:** map IPv6 unicast routes (MP_REACH/MP_UNREACH NLRI) ([#180](https://github.com/andrediashexa/NOGGlass/issues/180)) ([ad904f4](https://github.com/andrediashexa/NOGGlass/commit/ad904f41c72030f3947091b2f0cdd54d46690cea))
* **bgp:** read extended communities in the path model ([#193](https://github.com/andrediashexa/NOGGlass/issues/193)) ([052d836](https://github.com/andrediashexa/NOGGlass/commit/052d836dcd407088b2092b8039370335b18d04d6)), closes [#192](https://github.com/andrediashexa/NOGGlass/issues/192)
* **bgp:** RPKI-validate routes served from the local RIB ([#185](https://github.com/andrediashexa/NOGGlass/issues/185)) ([59f7b36](https://github.com/andrediashexa/NOGGlass/commit/59f7b3646679582d5ed211c1685c75fde2c81a4b))
* **bmp:** answer bgp_summary for a monitored router ([#191](https://github.com/andrediashexa/NOGGlass/issues/191)) ([1468307](https://github.com/andrediashexa/NOGGlass/commit/1468307178db229e1009d2f8ac60dcf9fcdb63fa)), closes [#190](https://github.com/andrediashexa/NOGGlass/issues/190)
* **infra:** accept YAML config files in addition to TOML ([#203](https://github.com/andrediashexa/NOGGlass/issues/203)) ([f8e4faa](https://github.com/andrediashexa/NOGGlass/commit/f8e4faa641d22a6ed3738035a02d4611cea7c87a)), closes [#202](https://github.com/andrediashexa/NOGGlass/issues/202)


### Documentation

* add a 1.0.0 readiness checklist ([#199](https://github.com/andrediashexa/NOGGlass/issues/199)) ([09d9030](https://github.com/andrediashexa/NOGGlass/commit/09d9030fce8791f8beb0aa02e482454a06d53689)), closes [#198](https://github.com/andrediashexa/NOGGlass/issues/198)
* add a project roadmap ([#206](https://github.com/andrediashexa/NOGGlass/issues/206)) ([dbdf6a3](https://github.com/andrediashexa/NOGGlass/commit/dbdf6a368265344220fb8f84252205efc5689261)), closes [#205](https://github.com/andrediashexa/NOGGlass/issues/205)
* **adr:** propose the web admin interface architecture (ADR-0017) ([#208](https://github.com/andrediashexa/NOGGlass/issues/208)) ([e46837e](https://github.com/andrediashexa/NOGGlass/commit/e46837e817254096e14ecb8c42320187f537e159))
* **readme:** show per-vendor verification status ([#197](https://github.com/andrediashexa/NOGGlass/issues/197)) ([fe73bd9](https://github.com/andrediashexa/NOGGlass/commit/fe73bd98b7db71e8f439e47cf064ddc4e4fad42e)), closes [#196](https://github.com/andrediashexa/NOGGlass/issues/196)


### Tests

* **frr:** verify the FRR driver against a real FRR 9.1.0 ([#201](https://github.com/andrediashexa/NOGGlass/issues/201)) ([5283fa6](https://github.com/andrediashexa/NOGGlass/commit/5283fa65bbcf8c2b161c9d3cd4baba649d955d84)), closes [#200](https://github.com/andrediashexa/NOGGlass/issues/200)


### Chores

* **bgp,bmp:** remove the embedded BGP session and BMP station ([#195](https://github.com/andrediashexa/NOGGlass/issues/195)) ([0483444](https://github.com/andrediashexa/NOGGlass/commit/04834448c3735dad92fd2370b4705c2bf8367a47)), closes [#194](https://github.com/andrediashexa/NOGGlass/issues/194)

## [0.7.0](https://github.com/andrediashexa/NOGGlass/compare/v0.6.5...v0.7.0) (2026-09-28)


### Features

* add bgp_aspath_v6 query and suppress graph/table when buffer is truncated ([e60ee81](https://github.com/andrediashexa/NOGGlass/commit/e60ee81f3ffa7a159d9416dc04fff4b71466e824))
* add optional bgp_aspath query, anchor AS-path regex to peer start, and ensure first-hop graph diversity ([5230d5e](https://github.com/andrediashexa/NOGGlass/commit/5230d5e0ffee6c84cc8c5637d9ba62763a07416a))
* address security and architecture audit findings ([baf3cee](https://github.com/andrediashexa/NOGGlass/commit/baf3ceec353a0cc52259536c67147659d681583b))
* **bgp:** NOGGlass's own BGP session as a query source ([#177](https://github.com/andrediashexa/NOGGlass/issues/177)) ([e797613](https://github.com/andrediashexa/NOGGlass/commit/e797613674cad20ca39e4f6953499f415d369d0c))
* **bmp:** a BMP station serving each monitored router's routes ([#178](https://github.com/andrediashexa/NOGGlass/issues/178)) ([45a7947](https://github.com/andrediashexa/NOGGlass/commit/45a794760a50b3270f4280d9efc998c212b84999))
* **config:** support nogglass.env natively in binary and standardize docker compose ([c959d06](https://github.com/andrediashexa/NOGGlass/commit/c959d06699d914e775f15e3131120151bf63963e))
* **driver:** add arista_eos and frr drivers with terminal length 0 ([8371688](https://github.com/andrediashexa/NOGGlass/commit/8371688ae36c8fdf06059f9cc9f38a08d222acc5))
* **governance:** switch to GPLv3, add project stack, governance docs and update UI footer link ([38045b3](https://github.com/andrediashexa/NOGGlass/commit/38045b3700c9acd31dc1984a0d4c7157aa17db04))
* **junos:** synthesize clean CLI text from JSON and hide display json in command header ([4628d54](https://github.com/andrediashexa/NOGGlass/commit/4628d5475df0f76554cb9209c00bad888e06bb15))
* **ssh:** add legacy sha1 kex algorithms as fallback-only ([8f33ae7](https://github.com/andrediashexa/NOGGlass/commit/8f33ae7ecfe5317fcdfe00f090fe6916b329eb44))
* **ui:** add show_best_path, show_paths and show_raw_output visibility settings ([ddb78ff](https://github.com/andrediashexa/NOGGlass/commit/ddb78ffa387ac21a93df6eb2728f542a40f01f9c))
* **ui:** add visual customization, light theme and bump version to 1.0.0 ([9f4498e](https://github.com/andrediashexa/NOGGlass/commit/9f4498eec8dd38e37add40d0dc71da51a8964b36))
* **ui:** highlight active BGP route in raw output and polish light mode diagram ([187e8fc](https://github.com/andrediashexa/NOGGlass/commit/187e8fcd8c17baeec767140b3d82c7b41cddd4e1))


### Bug Fixes

* **huawei:** ignore wrapped status legend lines and extend flag characters ([#168](https://github.com/andrediashexa/NOGGlass/issues/168)) ([cd277d4](https://github.com/andrediashexa/NOGGlass/commit/cd277d4b33bb8f3bf50f293f1f867db424aca494))
* **huawei:** parse multi-line IPv6 BGP routing table blocks ([ea32889](https://github.com/andrediashexa/NOGGlass/commit/ea32889ccbddb0dd741fdec31713a9e298ecaf87))
* **junos:** handle empty route responses and improve aspath-regex matching ([bcd9af1](https://github.com/andrediashexa/NOGGlass/commit/bcd9af132ffd40077db4a1afabbb36b285f38ffe))
* **junos:** handle inactive route empty tags, prefix length, and multiline as-path ([34d6452](https://github.com/andrediashexa/NOGGlass/commit/34d6452663b8efcf527fb92ebd424cc1d1c0b913))
* **junos:** prevent strip_echo from dropping JSON closing brace and handle unparseable output gracefully ([3891677](https://github.com/andrediashexa/NOGGlass/commit/38916776e08702f21e584cd0d4d826a1cdd213f5))
* **ratelimit:** set default_captcha_secs to 60 and document in example config ([#169](https://github.com/andrediashexa/NOGGlass/issues/169)) ([4271624](https://github.com/andrediashexa/NOGGlass/commit/4271624afc539d89631fedff12437055e9bceef0))
* **rpki:** prevent caching transient lookup failures and support Huawei detail RPKI ([d96bdc4](https://github.com/andrediashexa/NOGGlass/commit/d96bdc4e3cc926495c60238a6ed17c38ed06badb))
* **ssh:** enforce 2MB safety buffer limit to protect against OOM killer on massive BGP dumps ([b536bb6](https://github.com/andrediashexa/NOGGlass/commit/b536bb6c598d84175a749bbe162a0c984b7b9e26))
* **ssh:** increase greeting timeout and recover from delayed paging responses ([6504544](https://github.com/andrediashexa/NOGGlass/commit/65045446419f038b17eafa4e1b4387cd9baef416))
* **ssh:** resolve host key cross-contamination and add diagnostic logging ([ce58851](https://github.com/andrediashexa/NOGGlass/commit/ce5885184253c23d0c6bc5f6db88425614f6f729))
* **ui:** completely suppress raw output and global view when buffer is truncated ([39bc29a](https://github.com/andrediashexa/NOGGlass/commit/39bc29a5ac0c54c06c2b402a58aaba687c06f63b))
* **ui:** respect show_best_path and deduplicate topological AS paths ([4aaafcf](https://github.com/andrediashexa/NOGGlass/commit/4aaafcf9cdc0e311afec8aeaac8ef0fad9212dba))
* **ui:** use topological DAG ranking from router for AS-path graph layout ([3115933](https://github.com/andrediashexa/NOGGlass/commit/3115933a7beff6ddab98e70d0596a3bc8b3f03fd))


### Documentation

* add read-only router account provisioning section to INSTALL.md ([9048a1e](https://github.com/andrediashexa/NOGGlass/commit/9048a1ed565d9788c3eaa78dd566fdedf67dac0c))
* **adr:** propose one Rust engine for the BGP session and BMP station ([#173](https://github.com/andrediashexa/NOGGlass/issues/173)) ([1f97768](https://github.com/andrediashexa/NOGGlass/commit/1f977686597c8a4132b78f57c31b08154cfdb272))
* **adr:** record authenticated collaborative multi-tenant architecture ([#30](https://github.com/andrediashexa/NOGGlass/issues/30)) ([c41b13a](https://github.com/andrediashexa/NOGGlass/commit/c41b13ab10b7243f848cf6c4ae2d4e6c378e01f4))
* **config:** translate UI visibility comments to English in example toml ([3dab403](https://github.com/andrediashexa/NOGGlass/commit/3dab403ebd8ac29925af18cb1e9d9ab88015bb21))
* **infra:** add standalone INSTALL.md at root ([#159](https://github.com/andrediashexa/NOGGlass/issues/159)) ([4583b7e](https://github.com/andrediashexa/NOGGlass/commit/4583b7e10ec39fcde27110423027567fc973fe62))
* **infra:** document upgrade procedure with docker compose in INSTAL… ([#160](https://github.com/andrediashexa/NOGGlass/issues/160)) ([774f2ac](https://github.com/andrediashexa/NOGGlass/commit/774f2ac8f9641e4c0fa42aefa693465dfa18771e))
* **install:** isolate docker compose in named volume and add /healthz alias ([4f635bf](https://github.com/andrediashexa/NOGGlass/commit/4f635bfbb92a2265eee53f10548befa8896f8b96))
* **install:** update rpki default timeout to 3000ms in examples and adr ([e143930](https://github.com/andrediashexa/NOGGlass/commit/e1439303342e2f1106bc114a7a8b604b2f36205d))
* **operations:** expand deployment with compilation and systemd instructions ([#158](https://github.com/andrediashexa/NOGGlass/issues/158)) ([c878f5f](https://github.com/andrediashexa/NOGGlass/commit/c878f5fe812d22b2aa826ff0d333561f4af808f1))


### Tests

* **huawei:** sanitize test prefixes to RFC 5737 and private ASNs ([#170](https://github.com/andrediashexa/NOGGlass/issues/170)) ([ec7fc9e](https://github.com/andrediashexa/NOGGlass/commit/ec7fc9ee81d61a718163b54e989fd0f31dd379cf))


### CI

* **release:** add automated static linux amd64 binary build and packaging ([36a1c57](https://github.com/andrediashexa/NOGGlass/commit/36a1c57fa8997b9d8470faab5169762183a15157))


### Style

* restore workspace rustfmt and fix bool asserts ([#175](https://github.com/andrediashexa/NOGGlass/issues/175)) ([71c882f](https://github.com/andrediashexa/NOGGlass/commit/71c882f9b44df26cb8713fa50d3329c635b1e410))
* **ui:** fix notice and error text contrast in light mode ([0899c93](https://github.com/andrediashexa/NOGGlass/commit/0899c935171fc3d777bf0e8ba9dd8fa59a137337))
* **ui:** force high-contrast notice colors and cache-busting in light mode ([0c9a9db](https://github.com/andrediashexa/NOGGlass/commit/0c9a9db4630820f78b5f6e2f0f77b112e60aff1d))

## [0.6.5](https://github.com/andrediashexa/looking-glass/compare/v0.6.4...v0.6.5) (2026-09-23)


### Bug Fixes

* **ui:** draw a prepended path, and stop calling a drawing bug unreachable ([#153](https://github.com/andrediashexa/looking-glass/issues/153)) ([7662420](https://github.com/andrediashexa/looking-glass/commit/7662420989cb5b9504244f1b7741060741110ca3))


### Tests

* **lab:** one command that checks a router against what went wrong ([#150](https://github.com/andrediashexa/looking-glass/issues/150)) ([4ae2f41](https://github.com/andrediashexa/looking-glass/commit/4ae2f41f0d5dbfc1dd33e0ef198c8553315c3542))
* **mock:** answer with the shapes a real routing table has ([#155](https://github.com/andrediashexa/looking-glass/issues/155)) ([9cf0331](https://github.com/andrediashexa/looking-glass/commit/9cf03317ada4fa2a2567488ca2933afcaae2c8dd))

## [0.6.4](https://github.com/andrediashexa/looking-glass/compare/v0.6.3...v0.6.4) (2026-09-23)


### Bug Fixes

* **ssh:** stop cutting a traceroute off while the silent hop is silent ([#147](https://github.com/andrediashexa/looking-glass/issues/147)) ([5f6e50c](https://github.com/andrediashexa/looking-glass/commit/5f6e50c022671c78a22b3c5dbf469c15cdb75b5c))

## [0.6.3](https://github.com/andrediashexa/looking-glass/compare/v0.6.2...v0.6.3) (2026-09-23)


### Bug Fixes

* **catalogue:** send an IPv6 route query a Huawei accepts ([#139](https://github.com/andrediashexa/looking-glass/issues/139)) ([c655037](https://github.com/andrediashexa/looking-glass/commit/c6550373fc2b75fc504d57fe04f933a34b0dccf1))
* **mikrotik:** drop a RouterOS 6 flag the driver never read ([#141](https://github.com/andrediashexa/looking-glass/issues/141)) ([0a1a378](https://github.com/andrediashexa/looking-glass/commit/0a1a378915e9b48599431f9fd9531ba63a89aedf))
* **ssh:** ask RouterOS with an exec request, and read its sessions ([#145](https://github.com/andrediashexa/looking-glass/issues/145)) ([34f4737](https://github.com/andrediashexa/looking-glass/commit/34f47374ffc3d3f35f6ac384aa4312dc80395e21))
* **summary:** read an IOS-XR peer's AS instead of its speaker number ([#143](https://github.com/andrediashexa/looking-glass/issues/143)) ([aed54c2](https://github.com/andrediashexa/looking-glass/commit/aed54c21c16ec89c638b856ef87928a0d7d4bcb0))


### Documentation

* Datacom needs hardware, and that is not going to change ([#136](https://github.com/andrediashexa/looking-glass/issues/136)) ([65a9d44](https://github.com/andrediashexa/looking-glass/commit/65a9d449666ca2cbb76ee9971df303d95eebb7a8))

## [0.6.2](https://github.com/andrediashexa/looking-glass/compare/v0.6.1...v0.6.2) (2026-09-21)


### Bug Fixes

* **cisco:** read IOS-XE's route detail, and the table by its header ([#128](https://github.com/andrediashexa/looking-glass/issues/128)) ([ef07ac7](https://github.com/andrediashexa/looking-glass/commit/ef07ac704dd26592e5ad8b878405010b1a44ba44))
* **ssh:** offer the NIST curves, without which no Cisco is reachable ([#134](https://github.com/andrediashexa/looking-glass/issues/134)) ([9f54907](https://github.com/andrediashexa/looking-glass/commit/9f5490732f9caf62fe2406ddc940279bdb98f893))


### Documentation

* record what was verified, and how each lab was built ([#132](https://github.com/andrediashexa/looking-glass/issues/132)) ([062cf80](https://github.com/andrediashexa/looking-glass/commit/062cf8069c21711e676e229ce04b59481a4875c2))
* say which drivers have read a real router, and make it a rule ([#126](https://github.com/andrediashexa/looking-glass/issues/126)) ([76d2c42](https://github.com/andrediashexa/looking-glass/commit/76d2c42251c81810cc8d7f746717c567af8140eb))

## [0.6.1](https://github.com/andrediashexa/looking-glass/compare/v0.6.0...v0.6.1) (2026-09-21)


### Bug Fixes

* **bird:** read a real BIRD 2, which loses neither prefix nor session ([#123](https://github.com/andrediashexa/looking-glass/issues/123)) ([3bed1b6](https://github.com/andrediashexa/looking-glass/commit/3bed1b6719d3edd0cc908f507d41d9ca57fafbe5))
* **huawei:** a ping that answered is no longer reported as total loss ([#117](https://github.com/andrediashexa/looking-glass/issues/117)) ([acdeb9c](https://github.com/andrediashexa/looking-glass/commit/acdeb9c9922dd8307577ea784fc15eb778f97cd4))
* **huawei:** read a real NE40E, and stop calling a down session up ([#124](https://github.com/andrediashexa/looking-glass/issues/124)) ([3523031](https://github.com/andrediashexa/looking-glass/commit/35230315c10096d695fe8e19a70601967b72af9e))
* **mikrotik:** read what RouterOS 7 actually prints ([#111](https://github.com/andrediashexa/looking-glass/issues/111)) ([8da1bbb](https://github.com/andrediashexa/looking-glass/commit/8da1bbbf0b1240d54fe3c0beac4002e78f675f22))
* **ssh:** connect to a Huawei, which the default host key order cannot ([#119](https://github.com/andrediashexa/looking-glass/issues/119)) ([8fc0eb3](https://github.com/andrediashexa/looking-glass/commit/8fc0eb314fad33625d348b7c82931772567ebcab))
* **ssh:** wait for the greeting, or the banner is the answer ([#121](https://github.com/andrediashexa/looking-glass/issues/121)) ([f0ff5b9](https://github.com/andrediashexa/looking-glass/commit/f0ff5b9366ee966fd32d3b134f96a7790f9ffbe4))


### Tests

* **lab:** add a containerlab topology built to break parsers ([#109](https://github.com/andrediashexa/looking-glass/issues/109)) ([8af48a4](https://github.com/andrediashexa/looking-glass/commit/8af48a485015b4d5834f8e3eb21a86bdaca5b182))

## [0.6.0](https://github.com/andrediashexa/looking-glass/compare/v0.5.0...v0.6.0) (2026-09-20)


### Features

* **ops:** bring the documentation up with docker compose ([#106](https://github.com/andrediashexa/looking-glass/issues/106)) ([67a1567](https://github.com/andrediashexa/looking-glass/commit/67a156720147e65a4707da313a50e726cb2242a9))

## [0.5.0](https://github.com/andrediashexa/looking-glass/compare/v0.4.2...v0.5.0) (2026-09-20)


### Features

* **docs:** serve the documentation as a site in a container ([#104](https://github.com/andrediashexa/looking-glass/issues/104)) ([d790ba1](https://github.com/andrediashexa/looking-glass/commit/d790ba1c070e9e9670b556d34147b7c9e9a85d0c))
* **web:** run a query from a link, and capture the screens ([#96](https://github.com/andrediashexa/looking-glass/issues/96)) ([608aa2e](https://github.com/andrediashexa/looking-glass/commit/608aa2e9a9efaf7e92300fbc1281cde09ce417a4))


### Documentation

* **design:** describe the interface and capture a payload per state ([4bbfd3a](https://github.com/andrediashexa/looking-glass/commit/4bbfd3a52c40add01b2cae559450205339ff6007))
* show the product in the README and outline a wiki for using it ([#98](https://github.com/andrediashexa/looking-glass/issues/98)) ([0227f25](https://github.com/andrediashexa/looking-glass/commit/0227f25590df6b8d670601de77c6b2b9494bb633))
* **wiki:** write the ten pages that teach using NOGGlass ([#100](https://github.com/andrediashexa/looking-glass/issues/100)) ([d352349](https://github.com/andrediashexa/looking-glass/commit/d3523498de100fb097ec5d0279b146de1cd454ea))


### Chores

* **docs:** generate the GitHub wiki from docs/wiki ([#102](https://github.com/andrediashexa/looking-glass/issues/102)) ([2094e05](https://github.com/andrediashexa/looking-glass/commit/2094e05d73c50c34c2acfb2eb85e95d804e5ea56))

## [0.4.2](https://github.com/andrediashexa/looking-glass/compare/v0.4.1...v0.4.2) (2026-09-20)


### Bug Fixes

* **ci:** give the pre-release step a repository to act on ([#92](https://github.com/andrediashexa/looking-glass/issues/92)) ([2ffd5f4](https://github.com/andrediashexa/looking-glass/commit/2ffd5f4df6e776d1393f1c10ffb359948fdc891e))

## [0.4.1](https://github.com/andrediashexa/looking-glass/compare/v0.4.0...v0.4.1) (2026-09-20)


### Bug Fixes

* **infra:** strip at link time so the arm64 build completes ([#89](https://github.com/andrediashexa/looking-glass/issues/89)) ([da811f1](https://github.com/andrediashexa/looking-glass/commit/da811f163feb983333839765172a37ae0cdf3dbd))


### Documentation

* **readme:** describe the product as it is now ([#87](https://github.com/andrediashexa/looking-glass/issues/87)) ([96122ab](https://github.com/andrediashexa/looking-glass/commit/96122abd474c37a33332825061fa65bb99939dcb))

## [0.4.0](https://github.com/andrediashexa/looking-glass/compare/v0.3.0...v0.4.0) (2026-09-20)


### Features

* **infra:** cross-compile the arm64 image instead of emulating it ([#85](https://github.com/andrediashexa/looking-glass/issues/85)) ([901c97c](https://github.com/andrediashexa/looking-glass/commit/901c97c4a0c3402b9a8a79ec45f49d6cfdfde7cd))
* **vendor:** implement the Nokia SR OS and Datacom DmOS drivers ([#84](https://github.com/andrediashexa/looking-glass/issues/84)) ([2d9fea9](https://github.com/andrediashexa/looking-glass/commit/2d9fea998a275f54a8de1f8323fd9a7ca5e18f35))
* **vendor:** read BGP session summaries into peer rows ([#83](https://github.com/andrediashexa/looking-glass/issues/83)) ([6083f66](https://github.com/andrediashexa/looking-glass/commit/6083f6699233550b429df78ab13e18be493723b8))
* **vendor:** read traceroute hops for every vendor ([#82](https://github.com/andrediashexa/looking-glass/issues/82)) ([5952583](https://github.com/andrediashexa/looking-glass/commit/59525832adad45f6b484d5702c52b07bf9ae9169))


### Bug Fixes

* **api:** compare the global view even when the router has no route ([#80](https://github.com/andrediashexa/looking-glass/issues/80)) ([ec29b8b](https://github.com/andrediashexa/looking-glass/commit/ec29b8b7d7b572fb1642df593498f4e5806ecc9e))

## [0.3.0](https://github.com/andrediashexa/looking-glass/compare/v0.2.0...v0.3.0) (2026-09-20)


### Features

* **api:** compare the router answer with what the Internet announces ([#74](https://github.com/andrediashexa/looking-glass/issues/74)) ([19f4326](https://github.com/andrediashexa/looking-glass/commit/19f4326458dfe55f4d7e3e815c11da7854729912))
* **api:** limit how many queries one visitor may run ([#69](https://github.com/andrediashexa/looking-glass/issues/69)) ([6e08600](https://github.com/andrediashexa/looking-glass/commit/6e086004520bb01ea789a70b3d59d66a6fda851b))


### Bug Fixes

* **vendor:** mark only the active RouterOS route as best ([#73](https://github.com/andrediashexa/looking-glass/issues/73)) ([bb4bd04](https://github.com/andrediashexa/looking-glass/commit/bb4bd041489ebb8517308ddbd6dd3c88100a44fd))
* **vendor:** populate Cisco attributes and stop losing the AS path ([#70](https://github.com/andrediashexa/looking-glass/issues/70)) ([b6a45d7](https://github.com/andrediashexa/looking-glass/commit/b6a45d7d832e50de4627c3f0d32c7efdc34d7374))


### CI

* publish an amd64 image and smoke-test it before calling it published ([#72](https://github.com/andrediashexa/looking-glass/issues/72)) ([74405a5](https://github.com/andrediashexa/looking-glass/commit/74405a52af90f11b4c1f714b7e9885f9f9c6ec03))
* skip the Rust build when no Rust changed ([#67](https://github.com/andrediashexa/looking-glass/issues/67)) ([666c20b](https://github.com/andrediashexa/looking-glass/commit/666c20bd00f3436a6cb0af04be1dcbcd7816fdf5))

## [0.2.0](https://github.com/andrediashexa/looking-glass/compare/v0.1.0...v0.2.0) (2026-09-20)


### ⚠ BREAKING CHANGES

* **api:** normalise the BGP path model per ADR-0006 ([#52](https://github.com/andrediashexa/looking-glass/issues/52))

### Features

* **api:** normalise the BGP path model per ADR-0006 ([#52](https://github.com/andrediashexa/looking-glass/issues/52)) ([b32301e](https://github.com/andrediashexa/looking-glass/commit/b32301eff20ca9ca02e465dde02bdb53e69bf91c))
* **api:** serve the query API from an Axum server ([#58](https://github.com/andrediashexa/looking-glass/issues/58)) ([29ba121](https://github.com/andrediashexa/looking-glass/commit/29ba1219a90c03585401ffc33dc58bbc18424e9e))
* **api:** validate RPKI in two tiers with the router first ([#62](https://github.com/andrediashexa/looking-glass/issues/62)) ([8a02b2e](https://github.com/andrediashexa/looking-glass/commit/8a02b2e0732449414d88ce58b4a76f57a11d3d6f))
* **api:** validate the router inventory at startup ([#56](https://github.com/andrediashexa/looking-glass/issues/56)) ([987bc01](https://github.com/andrediashexa/looking-glass/commit/987bc0112695b9c58f55da800e118e04f9b9fbb6))
* **executor:** move router commands into a data catalogue ([#53](https://github.com/andrediashexa/looking-glass/issues/53)) ([4ae142d](https://github.com/andrediashexa/looking-glass/commit/4ae142df3d2254f3f8475fd600424c86293dd6ea))
* **executor:** run queries over SSH with limits and a mock path ([#57](https://github.com/andrediashexa/looking-glass/issues/57)) ([81c5c3b](https://github.com/andrediashexa/looking-glass/commit/81c5c3ba99580ee152d74bfa695e5418ab0dc9f5))
* **executor:** validate query targets and cap query output ([#50](https://github.com/andrediashexa/looking-glass/issues/50)) ([d13ef01](https://github.com/andrediashexa/looking-glass/commit/d13ef01c09b6a5c9faea4bee5aa97ab7446f6e1b))
* **infra:** ship a container image and document deployment ([#63](https://github.com/andrediashexa/looking-glass/issues/63)) ([c9b14a3](https://github.com/andrediashexa/looking-glass/commit/c9b14a3aa6a8831280e8a679de2565e66c568283))
* **vendor:** add a mock router with documentation-range fixtures ([#55](https://github.com/andrediashexa/looking-glass/issues/55)) ([ee85734](https://github.com/andrediashexa/looking-glass/commit/ee85734235c291ce4ea54fdb5207d892300d9247))
* **web:** embed the interface in the binary, in three languages ([#61](https://github.com/andrediashexa/looking-glass/issues/61)) ([4234743](https://github.com/andrediashexa/looking-glass/commit/4234743fc07d3e0d5f2541581d6be8f01b1fb950))


### Bug Fixes

* **vendor:** read Huawei VRP fields by type instead of column position ([#54](https://github.com/andrediashexa/looking-glass/issues/54)) ([db7bf37](https://github.com/andrediashexa/looking-glass/commit/db7bf37d892e08fa098d36f08622fd17ea58a9fd))


### Documentation

* **adr:** accept the design system and the tiered RPKI validation ([#49](https://github.com/andrediashexa/looking-glass/issues/49)) ([c5ca7b4](https://github.com/andrediashexa/looking-glass/commit/c5ca7b433ef56549c20761f492eb1bbb54cfc72b))
* **adr:** release every change and flag patch releases as pre-releases ([#60](https://github.com/andrediashexa/looking-glass/issues/60)) ([ade2899](https://github.com/andrediashexa/looking-glass/commit/ade28990ddd41f8e20b5718504e095e80a04db0d))
* **operations:** quote the memory figure that was actually measured ([#65](https://github.com/andrediashexa/looking-glass/issues/65)) ([bea748f](https://github.com/andrediashexa/looking-glass/commit/bea748fa0faa43c1ccddd824e1ca2762f75063f4))


### Chores

* **ci:** drop the component prefix from release tags ([#45](https://github.com/andrediashexa/looking-glass/issues/45)) ([2153931](https://github.com/andrediashexa/looking-glass/commit/2153931938bfc770b9497fabd85a560c2d3e0908))

## 0.1.0 (2026-09-20)


### Features

* **core:** implement structured parsers for ping traceroute bgp and … ([#22](https://github.com/andrediashexa/looking-glass/issues/22)) ([7de5af1](https://github.com/andrediashexa/looking-glass/commit/7de5af13380b523101aebd9441aa0bf6bb1fbae7))


### Bug Fixes

* **vendor:** stop returning fabricated ping, traceroute and origin data ([#43](https://github.com/andrediashexa/looking-glass/issues/43)) ([759f794](https://github.com/andrediashexa/looking-glass/commit/759f794be31952aa9e5d695963f4bc6d97a9eec6))
