//! The neighbour summary for a BMP-monitored router (#190).
//!
//! The router's peers are discovered from BMP itself (Peer Up / Peer Down and
//! Route Monitoring headers, tracked in [`crate::bmp::rib::BmpPeer`]); this
//! shapes them into the same [`BgpSummaryResult`] a vendor router gives, so a
//! BMP source answers `bgp_summary` like every other. Pure and testable — the
//! runtime and API hold the moving parts.
//!
//! Unlike the BGP session's summary, the router's own id and local AS are not
//! ours to state — BMP reports the monitored *peers*, not the router's
//! configuration — so `router_id` and `local_as` stay `None`.

use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::bgp::rib::LocalRib;
use crate::bmp::rib::BmpPeer;
use crate::driver::{BgpPeerSummary, BgpSummaryResult};

/// Builds a monitored router's neighbour summary from its peer table and RIB.
///
/// Each peer shows its AS, whether it is up (`Established`) or down (`Idle`), and
/// how many prefixes it has advertised into the router's RIB.
pub fn bmp_summary(peers: &BTreeMap<IpAddr, BmpPeer>, rib: &LocalRib) -> BgpSummaryResult {
    let peer_rows = peers
        .iter()
        .map(|(addr, peer)| BgpPeerSummary {
            peer_ip: addr.to_string(),
            peer_as: peer.asn,
            state: if peer.up { "Established" } else { "Idle" }.to_string(),
            uptime: String::new(),
            prefixes_received: rib.prefix_count_for_peer(*addr) as u32,
            prefixes_accepted: None,
        })
        .collect();

    BgpSummaryResult {
        router_id: None,
        local_as: None,
        peers: peer_rows,
        raw_output: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::BgpPath;

    #[test]
    fn lists_each_peer_with_its_as_state_and_prefix_count() {
        let mut peers = BTreeMap::new();
        peers.insert(
            "192.0.2.1".parse().unwrap(),
            BmpPeer {
                asn: 65001,
                up: true,
            },
        );
        peers.insert(
            "192.0.2.2".parse().unwrap(),
            BmpPeer {
                asn: 65002,
                up: false,
            },
        );

        let mut rib = LocalRib::new();
        rib.apply_update(
            "192.0.2.1".parse().unwrap(),
            vec![BgpPath {
                prefix: Some("203.0.113.0/24".parse().unwrap()),
                peer: Some("192.0.2.1".parse().unwrap()),
                ..Default::default()
            }],
            &[],
        );

        let summary = bmp_summary(&peers, &rib);
        // The router's own id/AS are not something BMP tells us.
        assert!(summary.router_id.is_none());
        assert!(summary.local_as.is_none());
        assert_eq!(summary.peers.len(), 2);

        let up = &summary.peers[0];
        assert_eq!(up.peer_ip, "192.0.2.1");
        assert_eq!(up.peer_as, 65001);
        assert_eq!(up.state, "Established");
        assert_eq!(up.prefixes_received, 1);

        let down = &summary.peers[1];
        assert_eq!(down.peer_ip, "192.0.2.2");
        assert_eq!(down.state, "Idle");
        assert_eq!(down.prefixes_received, 0);
    }

    #[test]
    fn a_router_with_no_peers_yet_summarises_to_an_empty_table() {
        let summary = bmp_summary(&BTreeMap::new(), &LocalRib::new());
        assert!(summary.peers.is_empty());
    }
}
