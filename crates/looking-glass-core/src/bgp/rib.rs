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

    /// Every path for a prefix, across peers, in a stable order (by peer).
    pub fn paths_for(&self, prefix: &IpNet) -> Vec<BgpPath> {
        self.by_prefix
            .get(prefix)
            .map(|peers| peers.values().cloned().collect())
            .unwrap_or_default()
    }

    /// The route query answer for `prefix`, as the normalised result the API
    /// already serves for the SSH drivers — every peer's path for it.
    ///
    /// A route learned over BGP has no raw router text, so `raw_output` is
    /// empty; the RIB either holds the prefix or it does not, so the answer is
    /// always [`Completeness::Complete`](crate::driver::Completeness) — an empty
    /// result means "no peer advertises this prefix", never "the lookup failed".
    pub fn route_result(&self, prefix: &IpNet) -> BgpRouteResult {
        BgpRouteResult::new(self.paths_for(prefix), String::new())
    }

    /// How many distinct prefixes the RIB holds.
    pub fn prefix_count(&self) -> usize {
        self.by_prefix.len()
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
}
