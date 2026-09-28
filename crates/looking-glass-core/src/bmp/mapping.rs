//! Turning a received BMP message into changes on the shared [`LocalRib`].
//!
//! BMP (RFC 7854) carries, per monitored peer, the BGP UPDATEs that peer sent
//! or received. NOGGlass already knows how to read a BGP UPDATE into the
//! normalised model ([`crate::bgp::mapping`]) and how to hold routes
//! ([`crate::bgp::rib`]); this module is the thin bridge that feeds the one into
//! the other. So a route seen over BMP reads exactly like a route seen over a
//! BGP session or scraped over SSH.
//!
//! Observe-only: this only ever records what a router reported. IPv4 unicast
//! only for now, matching the BGP mapper; other families arrive with it.

use std::net::IpAddr;

use netgauze_bgp_pkt::BgpMessage;
use netgauze_bmp_pkt::v3::BmpMessageValue;
use netgauze_bmp_pkt::BmpMessage;

use crate::bgp::mapping::{paths_from_update, withdrawn_from_update};
use crate::bgp::rib::LocalRib;
use crate::driver::BgpPath;

/// The paths a BMP Route Monitoring message reports for its monitored peer.
///
/// The message wraps a single BGP UPDATE; its NLRI become paths carrying the
/// monitored peer's address, exactly as [`paths_from_update`] would map a live
/// session's UPDATE. Returns nothing when the peer address is absent or the
/// wrapped message is not an UPDATE.
pub fn paths_from_route_monitoring(
    rm: &netgauze_bmp_pkt::v3::RouteMonitoringMessage,
) -> Vec<BgpPath> {
    let Some(peer) = rm.peer_header().address() else {
        return Vec::new();
    };
    match rm.update_message() {
        BgpMessage::Update(update) => paths_from_update(update, Some(peer)),
        _ => Vec::new(),
    }
}

/// Applies one received BMP message to `rib`.
///
/// - Route Monitoring installs its UPDATE's prefixes and drops its withdrawals,
///   under the monitored peer's address.
/// - Peer Down forgets every route learned from that peer, so the looking glass
///   stops showing a neighbour a router no longer monitors as up.
/// - Everything else (Initiation, Peer Up, Statistics, Termination, Route
///   Mirroring) changes no routes here.
///
/// BMP v4 messages are not yet mapped and are ignored rather than mishandled.
pub fn apply_bmp_message(rib: &mut LocalRib, message: &BmpMessage) {
    let BmpMessage::V3(value) = message else {
        return;
    };
    match value {
        BmpMessageValue::RouteMonitoring(rm) => {
            let Some(peer) = rm.peer_header().address() else {
                return;
            };
            if let BgpMessage::Update(update) = rm.update_message() {
                let advertised = paths_from_update(update, Some(peer));
                let withdrawn = withdrawn_from_update(update);
                rib.apply_update(peer, advertised, &withdrawn);
            }
        }
        BmpMessageValue::PeerDownNotification(pd) => {
            if let Some(peer) = peer_address(pd.peer_header()) {
                rib.remove_peer(peer);
            }
        }
        _ => {}
    }
}

/// The monitored peer's address from a header, if present.
fn peer_address(header: &netgauze_bmp_pkt::PeerHeader) -> Option<IpAddr> {
    header.address()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipnet::Ipv4Net;
    use netgauze_bgp_pkt::nlri::{Ipv4Unicast, Ipv4UnicastAddress};
    use netgauze_bgp_pkt::path_attribute::{
        As4PathSegment, AsPath, AsPathSegmentType, NextHop, PathAttribute, PathAttributeValue,
    };
    use netgauze_bgp_pkt::update::BgpUpdateMessage;
    use netgauze_bgp_pkt::BgpMessage;
    use netgauze_bmp_pkt::v3::{
        BmpMessageValue, PeerDownNotificationMessage, PeerDownNotificationReason,
        RouteMonitoringMessage,
    };
    use netgauze_bmp_pkt::{BmpMessage, BmpPeerType, PeerHeader};
    use std::net::Ipv4Addr;

    fn attr(value: PathAttributeValue) -> PathAttribute {
        let optional = value.can_be_optional().unwrap_or(false);
        let transitive = value.can_be_transitive().unwrap_or(true);
        let partial = value.can_be_partial().unwrap_or(false);
        PathAttribute::from(optional, transitive, partial, false, value).expect("valid attribute")
    }

    fn nlri(prefix: &str) -> Ipv4UnicastAddress {
        let net: Ipv4Net = prefix.parse().unwrap();
        Ipv4UnicastAddress::new_no_path_id(Ipv4Unicast::from_net(net).unwrap())
    }

    fn peer_header(addr: &str) -> PeerHeader {
        PeerHeader::new(
            BmpPeerType::GlobalInstancePeer {
                ipv6: false,
                post_policy: false,
                asn2: false,
                adj_rib_out: false,
            },
            None,
            Some(addr.parse().unwrap()),
            64496,
            Ipv4Addr::new(192, 0, 2, 254),
            None,
        )
    }

    fn route_monitoring(peer: &str, prefix: &str) -> BmpMessage {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                attr(PathAttributeValue::AsPath(AsPath::As4PathSegments(
                    vec![As4PathSegment::new(
                        AsPathSegmentType::AsSequence,
                        vec![65001],
                    )]
                    .into(),
                ))),
                attr(PathAttributeValue::NextHop(NextHop::new(Ipv4Addr::new(
                    192, 0, 2, 254,
                )))),
            ],
            vec![nlri(prefix)],
        );
        let rm = RouteMonitoringMessage::build(peer_header(peer), BgpMessage::Update(update))
            .expect("valid route monitoring");
        BmpMessage::V3(BmpMessageValue::RouteMonitoring(rm))
    }

    #[test]
    fn route_monitoring_installs_the_peers_routes() {
        let mut rib = LocalRib::new();
        apply_bmp_message(&mut rib, &route_monitoring("192.0.2.1", "203.0.113.0/24"));
        let paths = rib.paths_for(&"203.0.113.0/24".parse().unwrap());
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].peer, Some("192.0.2.1".parse().unwrap()));
        assert_eq!(paths[0].as_path, vec![65001]);
        assert_eq!(paths[0].next_hop, Some("192.0.2.254".parse().unwrap()));
    }

    #[test]
    fn two_monitored_peers_both_show_for_a_prefix() {
        let mut rib = LocalRib::new();
        apply_bmp_message(&mut rib, &route_monitoring("192.0.2.1", "203.0.113.0/24"));
        apply_bmp_message(&mut rib, &route_monitoring("192.0.2.2", "203.0.113.0/24"));
        assert_eq!(rib.paths_for(&"203.0.113.0/24".parse().unwrap()).len(), 2);
    }

    fn peer_down(peer: &str) -> BmpMessage {
        let pd = PeerDownNotificationMessage::build(
            peer_header(peer),
            PeerDownNotificationReason::PeerDeConfigured,
        )
        .expect("valid peer down");
        BmpMessage::V3(BmpMessageValue::PeerDownNotification(pd))
    }

    #[test]
    fn peer_down_forgets_only_that_peers_routes() {
        let mut rib = LocalRib::new();
        apply_bmp_message(&mut rib, &route_monitoring("192.0.2.1", "203.0.113.0/24"));
        apply_bmp_message(&mut rib, &route_monitoring("192.0.2.2", "203.0.113.0/24"));
        apply_bmp_message(&mut rib, &peer_down("192.0.2.1"));
        let paths = rib.paths_for(&"203.0.113.0/24".parse().unwrap());
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].peer, Some("192.0.2.2".parse().unwrap()));
    }

    #[test]
    fn paths_from_route_monitoring_reads_the_embedded_update() {
        let BmpMessage::V3(BmpMessageValue::RouteMonitoring(rm)) =
            route_monitoring("192.0.2.9", "198.51.100.0/24")
        else {
            unreachable!("constructed a route monitoring message");
        };
        let paths = paths_from_route_monitoring(&rm);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(paths[0].peer, Some("192.0.2.9".parse().unwrap()));
    }
}
