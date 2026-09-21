# Changelog

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
