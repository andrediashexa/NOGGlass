//! Driving what a peer tells us into the [`LocalRib`].
//!
//! NetGauze's speaker hands the application a stream of `(FsmState, BgpEvent)`
//! per peer. [`apply_bgp_event`] is the pure reducer over that stream: given one
//! event, it makes the one change to the RIB it implies, and nothing else. The
//! socket that produces the stream (the supervisor and per-peer task) is a
//! separate, thinner layer over this — kept apart so the routing logic is
//! testable without a live neighbour.

use std::net::IpAddr;

use netgauze_bgp_speaker::events::{BgpEvent, UpdateTreatment};

use crate::bgp::mapping::{paths_from_update, withdrawn_from_update};
use crate::bgp::rib::LocalRib;

/// Applies one BGP event from `peer` to `rib`.
///
/// Read-only toward the network: this only ever records what a peer sent.
///
/// - A normal UPDATE adds its advertised prefixes and drops its withdrawn ones.
/// - RFC 7606 *treat-as-withdraw*: a malformed UPDATE's reachable prefixes are
///   withdrawn rather than trusted, so a bad announcement removes a route
///   instead of installing a wrong one.
/// - A session that goes down — TCP failure, NOTIFICATION, hold timer, an
///   administrative or error-driven stop, or an error severe enough to reset
///   the session — drops every route learned from that peer, so the looking
///   glass stops showing a neighbour that is no longer up.
/// - Everything else (OPEN, KEEPALIVE, timers, route-refresh) changes no routes.
pub fn apply_bgp_event<A>(rib: &mut LocalRib, peer: IpAddr, event: &BgpEvent<A>) {
    match event {
        BgpEvent::UpdateMsg(update, treatment) => match treatment {
            UpdateTreatment::Normal | UpdateTreatment::AttributeDiscard => {
                let advertised = paths_from_update(update, Some(peer));
                let withdrawn = withdrawn_from_update(update);
                rib.apply_update(peer, advertised, &withdrawn);
            }
            UpdateTreatment::TreatAsWithdraw => {
                // The announcement is malformed; per RFC 7606 its reachable
                // prefixes are treated as withdrawals, never installed.
                let mut withdrawn = withdrawn_from_update(update);
                withdrawn.extend(
                    paths_from_update(update, Some(peer))
                        .into_iter()
                        .filter_map(|path| path.prefix),
                );
                rib.apply_update(peer, Vec::new(), &withdrawn);
            }
            // A family reset or a session reset: the safe reading is that this
            // peer's routes can no longer be trusted, so forget them.
            UpdateTreatment::ResetAddressFamily(_, _) | UpdateTreatment::SessionReset => {
                rib.remove_peer(peer);
            }
        },
        BgpEvent::TcpConnectionFails
        | BgpEvent::NotifMsg(_)
        | BgpEvent::NotifMsgErr(_)
        | BgpEvent::NotifMsgVerErr
        | BgpEvent::HoldTimerExpires
        | BgpEvent::AutomaticStop
        | BgpEvent::ManualStop => {
            rib.remove_peer(peer);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipnet::Ipv4Net;
    use netgauze_bgp_pkt::nlri::{Ipv4Unicast, Ipv4UnicastAddress};
    use netgauze_bgp_pkt::path_attribute::{
        As4PathSegment, AsPath, AsPathSegmentType, PathAttribute, PathAttributeValue,
    };
    use netgauze_bgp_pkt::update::BgpUpdateMessage;
    use std::net::SocketAddr;

    // The event address type is irrelevant to the RIB, so fix a concrete one.
    type Ev = BgpEvent<SocketAddr>;

    fn peer() -> IpAddr {
        "192.0.2.1".parse().unwrap()
    }

    fn nlri(prefix: &str) -> Ipv4UnicastAddress {
        let net: Ipv4Net = prefix.parse().unwrap();
        Ipv4UnicastAddress::new_no_path_id(Ipv4Unicast::from_net(net).unwrap())
    }

    fn as_path_attr(numbers: Vec<u32>) -> PathAttribute {
        let value = PathAttributeValue::AsPath(AsPath::As4PathSegments(
            vec![As4PathSegment::new(AsPathSegmentType::AsSequence, numbers)].into(),
        ));
        PathAttribute::from(false, true, false, false, value).unwrap()
    }

    fn advertise(prefix: &str) -> BgpUpdateMessage {
        BgpUpdateMessage::new(vec![], vec![as_path_attr(vec![65100])], vec![nlri(prefix)])
    }

    #[test]
    fn a_normal_update_installs_the_route() {
        let mut rib = LocalRib::new();
        let event: Ev = BgpEvent::UpdateMsg(advertise("198.51.100.0/24"), UpdateTreatment::Normal);
        apply_bgp_event(&mut rib, peer(), &event);
        assert_eq!(rib.paths_for(&"198.51.100.0/24".parse().unwrap()).len(), 1);
    }

    #[test]
    fn an_explicit_withdraw_removes_the_route() {
        let mut rib = LocalRib::new();
        let prefix = "198.51.100.0/24".parse().unwrap();
        apply_bgp_event(
            &mut rib,
            peer(),
            &(BgpEvent::UpdateMsg(advertise("198.51.100.0/24"), UpdateTreatment::Normal) as Ev),
        );
        // A withdraw-only UPDATE for the same prefix.
        let withdraw: Ev = BgpEvent::UpdateMsg(
            BgpUpdateMessage::new(vec![nlri("198.51.100.0/24")], vec![], vec![]),
            UpdateTreatment::Normal,
        );
        apply_bgp_event(&mut rib, peer(), &withdraw);
        assert!(rib.paths_for(&prefix).is_empty());
    }

    #[test]
    fn treat_as_withdraw_removes_rather_than_installs() {
        // RFC 7606: a malformed announcement withdraws its NLRI, never installs.
        let mut rib = LocalRib::new();
        let prefix = "198.51.100.0/24".parse().unwrap();
        // First install it normally from this peer.
        apply_bgp_event(
            &mut rib,
            peer(),
            &(BgpEvent::UpdateMsg(advertise("198.51.100.0/24"), UpdateTreatment::Normal) as Ev),
        );
        // Now the same prefix arrives malformed → treat-as-withdraw.
        let bad: Ev = BgpEvent::UpdateMsg(
            advertise("198.51.100.0/24"),
            UpdateTreatment::TreatAsWithdraw,
        );
        apply_bgp_event(&mut rib, peer(), &bad);
        assert!(
            rib.paths_for(&prefix).is_empty(),
            "a malformed announcement must remove the route, not install a wrong one"
        );
    }

    #[test]
    fn a_notification_drops_every_route_from_that_peer() {
        let mut rib = LocalRib::new();
        apply_bgp_event(
            &mut rib,
            peer(),
            &(BgpEvent::UpdateMsg(advertise("198.51.100.0/24"), UpdateTreatment::Normal) as Ev),
        );
        apply_bgp_event(
            &mut rib,
            peer(),
            &(BgpEvent::UpdateMsg(advertise("203.0.113.0/24"), UpdateTreatment::Normal) as Ev),
        );
        assert_eq!(rib.prefix_count(), 2);

        let down: Ev = BgpEvent::TcpConnectionFails;
        apply_bgp_event(&mut rib, peer(), &down);
        assert!(rib.is_empty(), "a session going down forgets its routes");
    }

    #[test]
    fn a_keepalive_changes_no_routes() {
        let mut rib = LocalRib::new();
        apply_bgp_event(
            &mut rib,
            peer(),
            &(BgpEvent::UpdateMsg(advertise("198.51.100.0/24"), UpdateTreatment::Normal) as Ev),
        );
        apply_bgp_event(&mut rib, peer(), &(BgpEvent::KeepAliveMsg as Ev));
        assert_eq!(rib.prefix_count(), 1, "a keepalive leaves the RIB alone");
    }
}
