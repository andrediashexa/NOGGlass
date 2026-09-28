//! Turning a received NetGauze UPDATE into the normalised [`BgpPath`] model.
//!
//! This is the typed bridge the whole feature exists for: routes arrive as
//! NetGauze structures, not as vendor text, so there is nothing to scrape and
//! no value to invent. An attribute the peer omitted stays `None` — a MED that
//! was never set is `None`, never `0` — which is the same contract the SSH
//! drivers hold ([ADR-0006](../../../docs/adr/0006-structured-bgp-model-and-data-sources.md)).

use std::net::IpAddr;

use ipnet::IpNet;
use netgauze_bgp_pkt::path_attribute::{AsPath, Origin as WireOrigin, PathAttributeValue};
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

/// Builds one [`BgpPath`] per advertised prefix in `update`.
///
/// `is_best` is `false`: best-path selection belongs to the RIB, not to a single
/// received UPDATE. `is_valid`/`rpki` stay at their defaults because BGP does
/// not carry RPKI. `peer` is whatever the caller knows about the session.
///
/// IPv4 unicast only for now. MP_REACH (IPv6 and other families) arrives in a
/// later change, so an UPDATE carrying only MP_REACH yields no paths here rather
/// than a wrong one.
pub fn paths_from_update(update: &BgpUpdateMessage, peer: Option<IpAddr>) -> Vec<BgpPath> {
    if update.nlri().is_empty() {
        return Vec::new();
    }

    let shared = read_shared(update);

    update
        .nlri()
        .iter()
        .map(|address| BgpPath {
            prefix: Some(IpNet::V4(address.network().address())),
            next_hop: shared.next_hop,
            peer,
            as_path: shared.as_path.clone(),
            local_pref: shared.local_pref,
            med: shared.med,
            origin: shared.origin,
            communities: shared.communities.clone(),
            ..BgpPath::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::CommunityKind;
    use ipnet::Ipv4Net;
    use netgauze_bgp_pkt::community::{Community as WireCommunity, LargeCommunity};
    use netgauze_bgp_pkt::nlri::{Ipv4Unicast, Ipv4UnicastAddress};
    use netgauze_bgp_pkt::path_attribute::{
        As4PathSegment, AsPath, AsPathSegmentType, Communities, LargeCommunities, LocalPreference,
        MultiExitDiscriminator, NextHop, PathAttribute, PathAttributeValue,
    };
    use std::net::Ipv4Addr;

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
}
