//! The routes NOGGlass has learned over its own BGP sessions, held in memory.
//!
//! One Adj-RIB-In per peer: what each peer advertised, exactly as received. A
//! query for a prefix returns every path across peers — which is what a looking
//! glass shows when you ask about a prefix, several paths from several
//! neighbours. Best-path selection is **not** done here (it is a later slice):
//! `is_best` stays as the mapper left it, so nothing here invents a decision the
//! session did not make.
//!
//! This type is not itself synchronised; the session task wraps it in a lock and
//! the API reads through that. Keeping it a plain structure keeps it testable.

use std::collections::BTreeMap;
use std::net::IpAddr;

use ipnet::IpNet;

use crate::driver::{BgpPath, BgpRouteResult};

/// The in-memory RIB: for each prefix, the path each peer advertised for it.
///
/// Prefix-first because the common read is "who has this prefix"; a peer going
/// down is rarer and sweeps every prefix once.
#[derive(Debug, Default)]
pub struct LocalRib {
    by_prefix: BTreeMap<IpNet, BTreeMap<IpAddr, BgpPath>>,
}

impl LocalRib {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one peer's UPDATE: the paths it now advertises (already mapped,
    /// each carrying its prefix) replace what that peer had for those prefixes,
    /// and the withdrawn prefixes are dropped for that peer.
    ///
    /// A path whose `prefix` is `None` (unreadable NLRI) is skipped rather than
    /// stored under a guessed key.
    pub fn apply_update(&mut self, peer: IpAddr, advertised: Vec<BgpPath>, withdrawn: &[IpNet]) {
        for path in advertised {
            let Some(prefix) = path.prefix else { continue };
            self.by_prefix.entry(prefix).or_default().insert(peer, path);
        }
        for prefix in withdrawn {
            self.withdraw(peer, *prefix);
        }
    }

    /// Removes one peer's path for a prefix, and forgets the prefix entirely
    /// once no peer advertises it.
    pub fn withdraw(&mut self, peer: IpAddr, prefix: IpNet) {
        if let Some(peers) = self.by_prefix.get_mut(&prefix) {
            peers.remove(&peer);
            if peers.is_empty() {
                self.by_prefix.remove(&prefix);
            }
        }
    }

    /// Drops everything a peer advertised. Called when a session goes down, so
    /// the looking glass stops showing routes learned from a neighbour that is
    /// no longer up.
    pub fn remove_peer(&mut self, peer: IpAddr) {
        self.by_prefix.retain(|_, peers| {
            peers.remove(&peer);
            !peers.is_empty()
        });
    }

    /// Every path for an **exact** prefix, across peers, in a stable order (by
    /// peer). Empty when the RIB holds no such prefix. For a route query prefer
    /// [`Self::route_result`], which matches the way a router answers.
    pub fn paths_for(&self, prefix: &IpNet) -> Vec<BgpPath> {
        self.by_prefix
            .get(prefix)
            .map(|peers| peers.values().cloned().collect())
            .unwrap_or_default()
    }

    /// The most specific prefix in the RIB that covers `query` — the longest
    /// prefix whose network contains the query address and whose length is no
    /// greater than the query's. `None` when nothing covers it.
    ///
    /// This is what makes a host or more-specific query behave like `show ip
    /// route <addr>`: walk candidate lengths from the query's down to 0 and take
    /// the first the RIB holds. A same-family lookup only — an IPv4 query never
    /// matches an IPv6 prefix.
    pub fn longest_match(&self, query: &IpNet) -> Option<IpNet> {
        let addr = query.addr();
        (0..=query.prefix_len()).rev().find_map(|len| {
            let candidate = IpNet::new(addr, len).ok()?.trunc();
            self.by_prefix.contains_key(&candidate).then_some(candidate)
        })
    }

    /// The route query answer for `query`, as the normalised result the API
    /// already serves for the SSH drivers — every peer's path for the route that
    /// covers it (longest-prefix match, [`Self::longest_match`]).
    ///
    /// A route learned over BGP has no raw router text, so `raw_output` is
    /// empty; the RIB either covers the query or it does not, so the answer is
    /// always [`Completeness::Complete`](crate::driver::Completeness) — an empty
    /// result means "no route covers this", never "the lookup failed".
    pub fn route_result(&self, query: &IpNet) -> BgpRouteResult {
        let paths = self
            .longest_match(query)
            .map(|prefix| self.paths_for(&prefix))
            .unwrap_or_default();
        BgpRouteResult::new(paths, String::new())
    }

    /// How many distinct prefixes the RIB holds.
    pub fn prefix_count(&self) -> usize {
        self.by_prefix.len()
    }

    /// How many prefixes a given peer advertises — the "prefixes received" a
    /// neighbour summary shows for that session.
    pub fn prefix_count_for_peer(&self, peer: IpAddr) -> usize {
        self.by_prefix
            .values()
            .filter(|peers| peers.contains_key(&peer))
            .count()
    }

    /// Whether the RIB holds nothing.
    pub fn is_empty(&self) -> bool {
        self.by_prefix.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(prefix: &str, peer: &str, as_path: Vec<u32>) -> BgpPath {
        BgpPath {
            prefix: Some(prefix.parse().expect("prefix")),
            peer: Some(peer.parse().expect("peer")),
            as_path,
            ..BgpPath::default()
        }
    }

    fn peer(ip: &str) -> IpAddr {
        ip.parse().expect("peer ip")
    }

    fn net(prefix: &str) -> IpNet {
        prefix.parse().expect("prefix")
    }

    #[test]
    fn an_advertised_route_can_be_queried_back() {
        let mut rib = LocalRib::new();
        rib.apply_update(
            peer("192.0.2.1"),
            vec![path("198.51.100.0/24", "192.0.2.1", vec![65100])],
            &[],
        );
        let paths = rib.paths_for(&net("198.51.100.0/24"));
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].as_path, vec![65100]);
        assert_eq!(rib.prefix_count(), 1);
    }

    #[test]
    fn two_peers_advertising_the_same_prefix_both_show() {
        let mut rib = LocalRib::new();
        rib.apply_update(
            peer("192.0.2.1"),
            vec![path("198.51.100.0/24", "192.0.2.1", vec![65100])],
            &[],
        );
        rib.apply_update(
            peer("192.0.2.2"),
            vec![path("198.51.100.0/24", "192.0.2.2", vec![65200])],
            &[],
        );
        let paths = rib.paths_for(&net("198.51.100.0/24"));
        assert_eq!(
            paths.len(),
            2,
            "a looking glass shows every neighbour's path"
        );
        // One prefix, two paths.
        assert_eq!(rib.prefix_count(), 1);
    }

    #[test]
    fn re_advertising_replaces_that_peers_path() {
        let mut rib = LocalRib::new();
        let p = peer("192.0.2.1");
        rib.apply_update(
            p,
            vec![path("198.51.100.0/24", "192.0.2.1", vec![65100])],
            &[],
        );
        rib.apply_update(
            p,
            vec![path("198.51.100.0/24", "192.0.2.1", vec![65100, 65999])],
            &[],
        );
        let paths = rib.paths_for(&net("198.51.100.0/24"));
        assert_eq!(
            paths.len(),
            1,
            "the same peer's newer path replaces the old"
        );
        assert_eq!(paths[0].as_path, vec![65100, 65999]);
    }

    #[test]
    fn a_withdraw_removes_only_that_peers_path() {
        let mut rib = LocalRib::new();
        let prefix = net("198.51.100.0/24");
        rib.apply_update(
            peer("192.0.2.1"),
            vec![path("198.51.100.0/24", "192.0.2.1", vec![65100])],
            &[],
        );
        rib.apply_update(
            peer("192.0.2.2"),
            vec![path("198.51.100.0/24", "192.0.2.2", vec![65200])],
            &[],
        );
        rib.apply_update(peer("192.0.2.1"), vec![], &[prefix]);
        let paths = rib.paths_for(&prefix);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].peer, Some(peer("192.0.2.2")));
    }

    #[test]
    fn withdrawing_the_last_path_forgets_the_prefix() {
        let mut rib = LocalRib::new();
        let prefix = net("198.51.100.0/24");
        rib.apply_update(
            peer("192.0.2.1"),
            vec![path("198.51.100.0/24", "192.0.2.1", vec![65100])],
            &[],
        );
        rib.apply_update(peer("192.0.2.1"), vec![], &[prefix]);
        assert!(rib.paths_for(&prefix).is_empty());
        assert!(rib.is_empty(), "no peer holds it, so the prefix is gone");
    }

    #[test]
    fn a_peer_going_down_drops_its_routes_and_leaves_the_rest() {
        let mut rib = LocalRib::new();
        rib.apply_update(
            peer("192.0.2.1"),
            vec![
                path("198.51.100.0/24", "192.0.2.1", vec![65100]),
                path("203.0.113.0/24", "192.0.2.1", vec![65100]),
            ],
            &[],
        );
        rib.apply_update(
            peer("192.0.2.2"),
            vec![path("198.51.100.0/24", "192.0.2.2", vec![65200])],
            &[],
        );

        rib.remove_peer(peer("192.0.2.1"));

        // 203.0.113.0/24 was only from the peer that left: gone.
        assert!(rib.paths_for(&net("203.0.113.0/24")).is_empty());
        // 198.51.100.0/24 still has the other peer's path.
        let remaining = rib.paths_for(&net("198.51.100.0/24"));
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].peer, Some(peer("192.0.2.2")));
        assert_eq!(rib.prefix_count(), 1);
    }

    #[test]
    fn an_unknown_prefix_returns_nothing() {
        let rib = LocalRib::new();
        assert!(rib.paths_for(&net("10.0.0.0/8")).is_empty());
    }

    #[test]
    fn a_path_with_no_prefix_is_skipped_not_stored() {
        let mut rib = LocalRib::new();
        let mut broken = path("198.51.100.0/24", "192.0.2.1", vec![65100]);
        broken.prefix = None;
        rib.apply_update(peer("192.0.2.1"), vec![broken], &[]);
        assert!(
            rib.is_empty(),
            "an unreadable NLRI is never stored under a guessed key"
        );
    }

    #[test]
    fn route_result_carries_the_paths_and_is_complete() {
        use crate::driver::Completeness;
        let mut rib = LocalRib::new();
        rib.apply_update(
            peer("192.0.2.1"),
            vec![path("198.51.100.0/24", "192.0.2.1", vec![65100])],
            &[],
        );
        let result = rib.route_result(&net("198.51.100.0/24"));
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.completeness, Completeness::Complete);
        assert!(
            result.raw_output.is_empty(),
            "a BGP-learned route has no raw router text"
        );
        assert!(!result.truncated);
    }

    #[test]
    fn route_result_for_an_unknown_prefix_is_empty_but_complete() {
        use crate::driver::Completeness;
        let rib = LocalRib::new();
        let result = rib.route_result(&net("10.0.0.0/8"));
        // Empty means "no peer advertises this", a real answer — not a failure.
        assert!(result.paths.is_empty());
        assert_eq!(result.completeness, Completeness::Complete);
    }

    /// Loads a RIB with one path per given prefix (all from one peer).
    fn rib_with(prefixes: &[&str]) -> LocalRib {
        let mut rib = LocalRib::new();
        for p in prefixes {
            rib.apply_update(
                peer("192.0.2.1"),
                vec![path(p, "192.0.2.1", vec![65100])],
                &[],
            );
        }
        rib
    }

    /// The single matched prefix a query resolves to, if any.
    fn matched(rib: &LocalRib, query: &str) -> Option<String> {
        let result = rib.route_result(&net(query));
        result.paths.first().map(|p| p.prefix.unwrap().to_string())
    }

    #[test]
    fn prefix_count_per_peer_counts_only_that_peers_prefixes() {
        let mut rib = LocalRib::new();
        rib.apply_update(
            peer("192.0.2.1"),
            vec![
                path("203.0.113.0/24", "192.0.2.1", vec![65100]),
                path("198.51.100.0/24", "192.0.2.1", vec![65100]),
            ],
            &[],
        );
        rib.apply_update(
            peer("192.0.2.2"),
            vec![path("203.0.113.0/24", "192.0.2.2", vec![65200])],
            &[],
        );
        assert_eq!(rib.prefix_count_for_peer(peer("192.0.2.1")), 2);
        assert_eq!(rib.prefix_count_for_peer(peer("192.0.2.2")), 1);
        assert_eq!(rib.prefix_count_for_peer(peer("192.0.2.9")), 0);
    }

    #[test]
    fn a_host_query_finds_the_covering_route() {
        let rib = rib_with(&["203.0.113.0/24"]);
        // 203.0.113.5/32 is not in the RIB, but the /24 covers it.
        assert_eq!(
            matched(&rib, "203.0.113.5/32"),
            Some("203.0.113.0/24".into())
        );
    }

    #[test]
    fn the_most_specific_covering_route_wins() {
        let rib = rib_with(&["203.0.113.0/24", "203.0.113.0/26", "0.0.0.0/0"]);
        // The /26 is the longest that covers 203.0.113.5.
        assert_eq!(
            matched(&rib, "203.0.113.5/32"),
            Some("203.0.113.0/26".into())
        );
    }

    #[test]
    fn an_exact_prefix_query_still_returns_itself() {
        let rib = rib_with(&["203.0.113.0/24", "198.51.100.0/24"]);
        assert_eq!(
            matched(&rib, "203.0.113.0/24"),
            Some("203.0.113.0/24".into())
        );
    }

    #[test]
    fn a_more_specific_query_returns_the_covering_route() {
        let rib = rib_with(&["203.0.113.0/24"]);
        // Nothing at /25, but the /24 covers 203.0.113.128/25.
        assert_eq!(
            matched(&rib, "203.0.113.128/25"),
            Some("203.0.113.0/24".into())
        );
    }

    #[test]
    fn a_default_route_covers_anything_when_nothing_more_specific_exists() {
        let rib = rib_with(&["0.0.0.0/0"]);
        assert_eq!(matched(&rib, "8.8.8.8/32"), Some("0.0.0.0/0".into()));
    }

    #[test]
    fn a_query_no_route_covers_is_empty() {
        let rib = rib_with(&["203.0.113.0/24"]);
        assert_eq!(matched(&rib, "8.8.8.8/32"), None);
    }

    #[test]
    fn longest_match_is_same_family_only() {
        let rib = rib_with(&["0.0.0.0/0"]);
        // An IPv6 host must not match the IPv4 default route.
        assert_eq!(matched(&rib, "2001:db8::1/128"), None);
    }

    #[test]
    fn ipv6_longest_prefix_match_works() {
        let rib = rib_with(&["2001:db8::/32", "2001:db8:100::/48"]);
        assert_eq!(
            matched(&rib, "2001:db8:100::5/128"),
            Some("2001:db8:100::/48".into())
        );
        // Outside the /48 but inside the /32.
        assert_eq!(
            matched(&rib, "2001:db8:200::1/128"),
            Some("2001:db8::/32".into())
        );
    }
}
