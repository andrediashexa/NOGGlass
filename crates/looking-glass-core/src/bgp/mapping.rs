//! Turning a received NetGauze UPDATE into the normalised [`BgpPath`] model.
//!
//! This is the typed bridge the whole feature exists for: routes arrive as
//! NetGauze structures, not as vendor text, so there is nothing to scrape and
//! no value to invent. An attribute the peer omitted stays `None` — a MED that
//! was never set is `None`, never `0` — which is the same contract the SSH
//! drivers hold ([ADR-0006](../../../docs/adr/0006-structured-bgp-model-and-data-sources.md)).

use std::net::IpAddr;

use ipnet::IpNet;
use netgauze_bgp_pkt::path_attribute::{
    AsPath, MpReach, MpUnreach, Origin as WireOrigin, PathAttributeValue,
};
use netgauze_bgp_pkt::update::BgpUpdateMessage;

use crate::driver::{BgpPath, Community, Origin};

/// The attributes an UPDATE shares across every prefix it advertises, read once.
#[derive(Default)]
struct SharedAttrs {
    next_hop: Option<IpAddr>,
    as_path: Vec<u32>,
    local_pref: Option<u32>,
    med: Option<u32>,
    origin: Option<Origin>,
    communities: Vec<Community>,
}

/// Flattens AS path segments into the flat list the model carries.
///
/// Two-byte ASNs widen to `u32` losslessly. AS_SET members are appended in
/// order like sequence members, because [`BgpPath::as_path`] is a flat list —
/// the same shape the vendor parsers produce.
fn flatten_as_path(as_path: &AsPath) -> Vec<u32> {
    let mut out = Vec::new();
    match as_path {
        AsPath::As2PathSegments(segments) => {
            for segment in segments.iter() {
                out.extend(segment.as_numbers().iter().map(|asn| u32::from(*asn)));
            }
        }
        AsPath::As4PathSegments(segments) => {
            for segment in segments.iter() {
                out.extend(segment.as_numbers().iter().copied());
            }
        }
    }
    out
}

fn read_shared(update: &BgpUpdateMessage) -> SharedAttrs {
    let mut shared = SharedAttrs::default();
    let mut as4_path: Option<Vec<u32>> = None;

    for attribute in update.path_attributes() {
        match attribute.value() {
            PathAttributeValue::Origin(origin) => {
                shared.origin = Some(match origin {
                    WireOrigin::IGP => Origin::Igp,
                    WireOrigin::EGP => Origin::Egp,
                    WireOrigin::Incomplete => Origin::Incomplete,
                });
            }
            PathAttributeValue::AsPath(path) => shared.as_path = flatten_as_path(path),
            PathAttributeValue::As4Path(path) => {
                let mut numbers = Vec::new();
                for segment in path.segments() {
                    numbers.extend(segment.as_numbers().iter().copied());
                }
                as4_path = Some(numbers);
            }
            PathAttributeValue::NextHop(next_hop) => {
                let ip = IpAddr::V4(next_hop.next_hop());
                // 0.0.0.0 is a locally originated marker, not a reachable hop.
                shared.next_hop = (!ip.is_unspecified()).then_some(ip);
            }
            PathAttributeValue::MultiExitDiscriminator(med) => shared.med = Some(med.metric()),
            PathAttributeValue::LocalPreference(pref) => shared.local_pref = Some(pref.metric()),
            PathAttributeValue::Communities(communities) => {
                for community in communities.communities() {
                    shared
                        .communities
                        .push(Community::parse(&community.to_string()));
                }
            }
            PathAttributeValue::LargeCommunities(communities) => {
                for community in communities.communities() {
                    shared.communities.push(Community::parse(&format!(
                        "{}:{}:{}",
                        community.global_admin(),
                        community.local_data1(),
                        community.local_data2()
                    )));
                }
            }
            // Extended communities (RFC 4360): route-target, route-origin, … .
            // NetGauze renders each canonically (`rt:65000:100`, `ro:…`); the
            // model classifies those as `Extended` by their shape.
            PathAttributeValue::ExtendedCommunities(communities) => {
                for community in communities.communities() {
                    shared
                        .communities
                        .push(Community::parse(&community.to_string()));
                }
            }
            _ => {}
        }
    }

    // RFC 6793: when an old two-byte speaker is in the path, AS_PATH carries
    // AS_TRANS (23456) where the real four-byte ASN sits in AS4_PATH. Prefer the
    // four-byte view so the true ASN survives rather than the placeholder.
    if let Some(numbers) = as4_path {
        if shared.as_path.is_empty() || shared.as_path.contains(&23456) {
            shared.as_path = numbers;
        }
    }

    shared
}

/// Assembles one [`BgpPath`] from a prefix, its next-hop, and the shared
/// attributes, so the IPv4 and IPv6 paths of one UPDATE read identically.
fn make_path(
    prefix: IpNet,
    next_hop: Option<IpAddr>,
    peer: Option<IpAddr>,
    shared: &SharedAttrs,
) -> BgpPath {
    BgpPath {
        prefix: Some(prefix),
        next_hop,
        peer,
        as_path: shared.as_path.clone(),
        local_pref: shared.local_pref,
        med: shared.med,
        origin: shared.origin,
        communities: shared.communities.clone(),
        ..BgpPath::default()
    }
}

/// A next-hop, dropping the unspecified address (0.0.0.0 / ::) that marks a
/// locally-originated route rather than a reachable hop.
fn reachable_next_hop(ip: IpAddr) -> Option<IpAddr> {
    (!ip.is_unspecified()).then_some(ip)
}

/// Builds one [`BgpPath`] per advertised prefix in `update`, across IPv4 unicast
/// (classic NLRI) and IPv6 unicast (MP_REACH_NLRI, RFC 4760).
///
/// `is_best` is `false`: best-path selection belongs to the RIB, not to a single
/// received UPDATE. `is_valid`/`rpki` stay at their defaults because BGP does
/// not carry RPKI. `peer` is whatever the caller knows about the session.
///
/// The path attributes (AS_PATH, ORIGIN, MED, LOCAL_PREF, communities) are
/// shared across both families; the next-hop differs by family — IPv4 carries it
/// in the NEXT_HOP attribute, IPv6 inside MP_REACH. MP families other than IPv6
/// unicast yield no paths here rather than a wrong one (fail closed).
pub fn paths_from_update(update: &BgpUpdateMessage, peer: Option<IpAddr>) -> Vec<BgpPath> {
    let shared = read_shared(update);
    let mut paths = Vec::new();

    // IPv4 unicast: classic NLRI with the NEXT_HOP attribute read into `shared`.
    for address in update.nlri() {
        paths.push(make_path(
            IpNet::V4(address.network().address()),
            shared.next_hop,
            peer,
            &shared,
        ));
    }

    // IPv6 unicast: MP_REACH_NLRI bundles the NLRI with their own next-hop.
    for attribute in update.path_attributes() {
        if let PathAttributeValue::MpReach(MpReach::Ipv6Unicast {
            next_hop_global,
            nlri,
            ..
        }) = attribute.value()
        {
            let next_hop = reachable_next_hop(IpAddr::V6(*next_hop_global));
            for address in nlri.iter() {
                paths.push(make_path(
                    IpNet::V6(address.network().address()),
                    next_hop,
                    peer,
                    &shared,
                ));
            }
        }
    }

    paths
}

/// The prefixes an UPDATE withdraws, for the RIB to drop, across IPv4 unicast
/// (classic withdrawals) and IPv6 unicast (MP_UNREACH_NLRI).
pub fn withdrawn_from_update(update: &BgpUpdateMessage) -> Vec<IpNet> {
    let mut withdrawn: Vec<IpNet> = update
        .withdraw_routes()
        .iter()
        .map(|address| IpNet::V4(address.network().address()))
        .collect();

    for attribute in update.path_attributes() {
        if let PathAttributeValue::MpUnreach(MpUnreach::Ipv6Unicast { nlri }) = attribute.value() {
            withdrawn.extend(
                nlri.iter()
                    .map(|address| IpNet::V6(address.network().address())),
            );
        }
    }

    withdrawn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::CommunityKind;
    use ipnet::{Ipv4Net, Ipv6Net};
    use netgauze_bgp_pkt::community::{
        Community as WireCommunity, ExtendedCommunity, LargeCommunity,
        TransitiveTwoOctetExtendedCommunity,
    };
    use netgauze_bgp_pkt::nlri::{
        Ipv4Unicast, Ipv4UnicastAddress, Ipv6Unicast, Ipv6UnicastAddress,
    };
    use netgauze_bgp_pkt::path_attribute::{
        As4PathSegment, AsPath, AsPathSegmentType, Communities, LargeCommunities, LocalPreference,
        MpReach, MpUnreach, MultiExitDiscriminator, NextHop, PathAttribute, PathAttributeValue,
    };
    use std::net::Ipv4Addr;

    fn nlri6(prefix: &str) -> Ipv6UnicastAddress {
        let net: Ipv6Net = prefix.parse().expect("prefix");
        Ipv6UnicastAddress::new(None, Ipv6Unicast::from_net(net).expect("unicast"))
    }

    /// An MP_REACH_NLRI for IPv6 unicast: NLRI bundled with their next-hop.
    fn mp_reach6(next_hop: &str, prefixes: &[&str]) -> PathAttributeValue {
        PathAttributeValue::MpReach(MpReach::Ipv6Unicast {
            next_hop_global: next_hop.parse().expect("v6 next-hop"),
            next_hop_local: None,
            nlri: prefixes.iter().map(|p| nlri6(p)).collect(),
        })
    }

    /// An MP_UNREACH_NLRI withdrawing IPv6 unicast prefixes.
    fn mp_unreach6(prefixes: &[&str]) -> PathAttributeValue {
        PathAttributeValue::MpUnreach(MpUnreach::Ipv6Unicast {
            nlri: prefixes.iter().map(|p| nlri6(p)).collect(),
        })
    }

    /// Wraps a value with the flags its own type says are valid, so a test does
    /// not have to know each attribute's optional/transitive bits.
    fn attr(value: PathAttributeValue) -> PathAttribute {
        let optional = value.can_be_optional().unwrap_or(false);
        let transitive = value.can_be_transitive().unwrap_or(true);
        let partial = value.can_be_partial().unwrap_or(false);
        PathAttribute::from(optional, transitive, partial, false, value).expect("valid attribute")
    }

    fn nlri(prefix: &str) -> Ipv4UnicastAddress {
        let net: Ipv4Net = prefix.parse().expect("prefix");
        Ipv4UnicastAddress::new_no_path_id(Ipv4Unicast::from_net(net).expect("unicast"))
    }

    fn as_seq(numbers: Vec<u32>) -> PathAttributeValue {
        PathAttributeValue::AsPath(AsPath::As4PathSegments(
            vec![As4PathSegment::new(AsPathSegmentType::AsSequence, numbers)].into(),
        ))
    }

    #[test]
    fn a_plain_update_becomes_one_path_with_its_attributes() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                attr(PathAttributeValue::Origin(WireOrigin::IGP)),
                attr(as_seq(vec![65100, 65500])),
                attr(PathAttributeValue::NextHop(NextHop::new(Ipv4Addr::new(
                    192, 0, 2, 254,
                )))),
                attr(PathAttributeValue::LocalPreference(LocalPreference::new(
                    150,
                ))),
                attr(PathAttributeValue::MultiExitDiscriminator(
                    MultiExitDiscriminator::new(10),
                )),
            ],
            vec![nlri("198.51.100.0/24")],
        );

        let peer = "192.0.2.1".parse().ok();
        let paths = paths_from_update(&update, peer);

        assert_eq!(paths.len(), 1);
        let path = &paths[0];
        assert_eq!(path.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(path.next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(path.as_path, vec![65100, 65500]);
        assert_eq!(path.origin_as(), Some(65500));
        assert_eq!(path.local_pref, Some(150));
        assert_eq!(path.med, Some(10));
        assert_eq!(path.origin, Some(Origin::Igp));
        assert_eq!(path.peer, peer);
        // The RIB decides best, not a lone UPDATE.
        assert!(!path.is_best);
    }

    #[test]
    fn a_missing_med_is_none_not_zero() {
        // The heart of ADR-0006: an attribute the peer never sent is absent.
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                attr(PathAttributeValue::Origin(WireOrigin::IGP)),
                attr(as_seq(vec![65100])),
            ],
            vec![nlri("203.0.113.0/24")],
        );
        let path = &paths_from_update(&update, None)[0];
        assert_eq!(path.med, None, "a MED nobody set MUST be None, never 0");
        assert_eq!(path.local_pref, None);
    }

    #[test]
    fn a_thirty_two_bit_asn_survives_the_path() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![attr(as_seq(vec![65100, 65536, 4_200_000_000]))],
            vec![nlri("203.0.113.0/24")],
        );
        let path = &paths_from_update(&update, None)[0];
        assert_eq!(path.as_path, vec![65100, 65536, 4_200_000_000]);
    }

    #[test]
    fn standard_and_large_communities_are_classified() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                attr(as_seq(vec![65100])),
                attr(PathAttributeValue::Communities(Communities::new(vec![
                    // 65001:100 encoded as a 32-bit value.
                    WireCommunity::new((65001 << 16) | 100),
                ]))),
                attr(PathAttributeValue::LargeCommunities(LargeCommunities::new(
                    vec![LargeCommunity::new(65001, 1, 2)],
                ))),
            ],
            vec![nlri("203.0.113.0/24")],
        );
        let path = &paths_from_update(&update, None)[0];
        let raws: Vec<&str> = path.communities.iter().map(|c| c.raw.as_str()).collect();
        assert_eq!(raws, vec!["65001:100", "65001:1:2"]);
        assert_eq!(path.communities[0].kind, CommunityKind::Standard);
        assert_eq!(path.communities[1].kind, CommunityKind::Large);
    }

    #[test]
    fn extended_communities_are_read_as_route_targets_and_origins() {
        use netgauze_bgp_pkt::path_attribute::ExtendedCommunities;
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                attr(as_seq(vec![65100])),
                attr(PathAttributeValue::ExtendedCommunities(
                    ExtendedCommunities::new(vec![
                        ExtendedCommunity::TransitiveTwoOctet(
                            TransitiveTwoOctetExtendedCommunity::RouteTarget {
                                global_admin: 65000,
                                local_admin: 100,
                            },
                        ),
                        ExtendedCommunity::TransitiveTwoOctet(
                            TransitiveTwoOctetExtendedCommunity::RouteOrigin {
                                global_admin: 65000,
                                local_admin: 7,
                            },
                        ),
                    ]),
                )),
            ],
            vec![nlri("203.0.113.0/24")],
        );
        let path = &paths_from_update(&update, None)[0];
        let raws: Vec<&str> = path.communities.iter().map(|c| c.raw.as_str()).collect();
        assert_eq!(raws, vec!["rt:65000:100", "ro:65000:7"]);
        assert!(path
            .communities
            .iter()
            .all(|c| c.kind == CommunityKind::Extended));
    }

    #[test]
    fn every_nlri_in_one_update_becomes_a_path() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![attr(as_seq(vec![65100]))],
            vec![nlri("198.51.100.0/24"), nlri("203.0.113.0/24")],
        );
        let paths = paths_from_update(&update, None);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(paths[1].prefix.unwrap().to_string(), "203.0.113.0/24");
        // The shared AS path is on both.
        assert!(paths.iter().all(|p| p.as_path == vec![65100]));
    }

    #[test]
    fn an_update_with_no_nlri_yields_no_paths() {
        // A withdraw-only or keepalive-shaped UPDATE advertises nothing.
        let update = BgpUpdateMessage::new(vec![], vec![], vec![]);
        assert!(paths_from_update(&update, None).is_empty());
    }

    #[test]
    fn a_zero_next_hop_is_not_a_reachable_hop() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                attr(as_seq(vec![65100])),
                attr(PathAttributeValue::NextHop(NextHop::new(Ipv4Addr::new(
                    0, 0, 0, 0,
                )))),
            ],
            vec![nlri("203.0.113.0/24")],
        );
        let path = &paths_from_update(&update, None)[0];
        assert_eq!(path.next_hop, None);
    }

    #[test]
    fn withdrawn_routes_are_read_out() {
        let update = BgpUpdateMessage::new(
            vec![nlri("198.51.100.0/24"), nlri("203.0.113.0/24")],
            vec![],
            vec![],
        );
        let withdrawn = withdrawn_from_update(&update);
        let shown: Vec<String> = withdrawn.iter().map(|n| n.to_string()).collect();
        assert_eq!(shown, vec!["198.51.100.0/24", "203.0.113.0/24"]);
        // A withdraw-only UPDATE advertises nothing.
        assert!(paths_from_update(&update, None).is_empty());
    }

    #[test]
    fn an_ipv6_update_becomes_a_path_with_the_mp_reach_next_hop() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                attr(PathAttributeValue::Origin(WireOrigin::IGP)),
                attr(as_seq(vec![65100, 65500])),
                attr(mp_reach6("2001:db8::1", &["2001:db8:100::/48"])),
            ],
            vec![],
        );
        let paths = paths_from_update(&update, None);
        assert_eq!(paths.len(), 1);
        let path = &paths[0];
        assert_eq!(path.prefix.unwrap().to_string(), "2001:db8:100::/48");
        assert_eq!(path.next_hop, Some("2001:db8::1".parse().unwrap()));
        // The shared attributes apply to the v6 path just like a v4 one.
        assert_eq!(path.as_path, vec![65100, 65500]);
        assert_eq!(path.origin, Some(Origin::Igp));
    }

    #[test]
    fn one_mp_reach_carrying_several_prefixes_yields_a_path_each() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![attr(mp_reach6(
                "2001:db8::1",
                &["2001:db8:1::/48", "2001:db8:2::/48"],
            ))],
            vec![],
        );
        let shown: Vec<String> = paths_from_update(&update, None)
            .iter()
            .map(|p| p.prefix.unwrap().to_string())
            .collect();
        assert_eq!(shown, vec!["2001:db8:1::/48", "2001:db8:2::/48"]);
    }

    #[test]
    fn a_dual_stack_update_yields_both_families() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                attr(as_seq(vec![65100])),
                attr(PathAttributeValue::NextHop(NextHop::new(Ipv4Addr::new(
                    192, 0, 2, 254,
                )))),
                attr(mp_reach6("2001:db8::1", &["2001:db8:100::/48"])),
            ],
            vec![nlri("203.0.113.0/24")],
        );
        let paths = paths_from_update(&update, None);
        assert_eq!(paths.len(), 2);
        let v4 = paths
            .iter()
            .find(|p| p.prefix.unwrap().addr().is_ipv4())
            .unwrap();
        let v6 = paths
            .iter()
            .find(|p| p.prefix.unwrap().addr().is_ipv6())
            .unwrap();
        assert_eq!(v4.next_hop, Some("192.0.2.254".parse().unwrap()));
        assert_eq!(v6.next_hop, Some("2001:db8::1".parse().unwrap()));
        // Both carry the one shared AS path.
        assert_eq!(v4.as_path, vec![65100]);
        assert_eq!(v6.as_path, vec![65100]);
    }

    #[test]
    fn an_unspecified_ipv6_next_hop_is_absent_not_the_all_zeros_address() {
        let update = BgpUpdateMessage::new(
            vec![],
            vec![attr(mp_reach6("::", &["2001:db8:100::/48"]))],
            vec![],
        );
        let path = &paths_from_update(&update, None)[0];
        assert_eq!(path.next_hop, None);
    }

    #[test]
    fn mp_unreach_ipv6_withdrawals_are_read_out() {
        let update = BgpUpdateMessage::new(
            vec![nlri("198.51.100.0/24")],
            vec![attr(mp_unreach6(&["2001:db8:1::/48", "2001:db8:2::/48"]))],
            vec![],
        );
        let shown: Vec<String> = withdrawn_from_update(&update)
            .iter()
            .map(|n| n.to_string())
            .collect();
        // IPv4 classic withdrawals and IPv6 MP_UNREACH withdrawals together.
        assert_eq!(
            shown,
            vec!["198.51.100.0/24", "2001:db8:1::/48", "2001:db8:2::/48"]
        );
    }

    #[test]
    fn a_non_ipv6_unicast_mp_family_yields_no_paths() {
        // MP_REACH for IPv4 multicast (a family we do not serve) must not be
        // mistaken for a route: fail closed, not a wrong guess.
        use netgauze_bgp_pkt::nlri::{Ipv4Multicast, Ipv4MulticastAddress};
        let net: Ipv4Net = "233.252.0.0/24".parse().unwrap();
        let update = BgpUpdateMessage::new(
            vec![],
            vec![attr(PathAttributeValue::MpReach(MpReach::Ipv4Multicast {
                next_hop: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 254)),
                next_hop_local: None,
                nlri: vec![Ipv4MulticastAddress::new_no_path_id(
                    Ipv4Multicast::from_net(net).unwrap(),
                )]
                .into(),
            }))],
            vec![],
        );
        assert!(paths_from_update(&update, None).is_empty());
    }
}
