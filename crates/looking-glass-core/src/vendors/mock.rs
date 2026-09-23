//! A router that does not exist, so the rest of the product can be built and
//! demonstrated without one.
//!
//! The mock answers from fixtures rather than from a network, which makes it
//! useful in three places: developing the interface, running CI, and the public
//! demo instance where pointing at real routers would be irresponsible.
//!
//! Every address in these fixtures comes from a range reserved for
//! documentation, and every AS number from the private range: RFC 5737
//! (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`), RFC 3849
//! (`2001:db8::/32`) and RFC 6996 (`64512`-`65534`). Nothing here can be
//! mistaken for a real network, and nothing here can send traffic to one.
//!
//! The mock is honest about being a mock: it answers only for the prefixes it
//! knows, and reports the same "no route" and "unsupported" outcomes a real
//! driver would, so the interface is exercised against the awkward cases too.

use crate::driver::{
    BgpPath, BgpRouteResult, BgpSummaryResult, Community, Completeness, DriverError, Origin,
    PingResult, QueryTarget, RpkiStatus, RpkiValidation, TracerouteHop, TracerouteResult,
    VendorDriver,
};
use ipnet::IpNet;
use std::net::IpAddr;

/// Identifier under which the mock appears in the catalogue and the inventory.
pub const MOCK_VENDOR: &str = "mock";

/// A fabricated router, for development, CI and the public demo.
///
/// It is not registered as a normal vendor: an operator has to enable it
/// explicitly, and the interface MUST state that the data is fabricated
/// whenever it is in use.
#[derive(Debug, Default, Clone, Copy)]
pub struct MockDriver;

/// The prefix the mock has a full story for.
fn documentation_prefix() -> IpNet {
    "198.51.100.0/24".parse().expect("RFC 5737 prefix")
}

/// A second prefix, RPKI invalid, so the interface can be built against the
/// alarming case and not only the happy one.
fn hijacked_prefix() -> IpNet {
    "203.0.113.0/24".parse().expect("RFC 5737 prefix")
}

/// An aggregate, so the interface meets an answer it cannot fully model: its
/// AS path stops at an AS_SET and the result says it is partial.
///
/// IPv6, because only a single /24 of each IPv4 documentation range is
/// reserved and an aggregate has to be shorter than its components — a /16
/// around 198.51.100.0/24 covers a great deal of space that belongs to
/// somebody. Every address this mock prints is reserved, and the test beside
/// it is what caught the first attempt.
fn aggregate_prefix() -> IpNet {
    "2001:db8:a::/48".parse().expect("RFC 3849 prefix")
}

/// The IPv6 prefix, RFC 3849.
fn documentation_prefix_v6() -> IpNet {
    "2001:db8::/32".parse().expect("RFC 3849 prefix")
}

fn addr(text: &str) -> IpAddr {
    text.parse().expect("fixture address")
}

impl MockDriver {
    /// Paths for a prefix the mock knows, or an empty list.
    fn paths_for(&self, target: &QueryTarget) -> Vec<BgpPath> {
        let wanted = match target {
            QueryTarget::Prefix(net) => *net,
            QueryTarget::Ip(ip) => {
                // Answer for the covering documentation prefix, the way a
                // router answers a longest-match lookup.
                let candidates = [
                    documentation_prefix(),
                    hijacked_prefix(),
                    documentation_prefix_v6(),
                ];
                match candidates.into_iter().find(|net| net.contains(ip)) {
                    Some(net) => net,
                    None => return Vec::new(),
                }
            }
            QueryTarget::Asn(asn) => {
                // The mock's transit ASes and its origin AS.
                if [65100, 65200, 65500].contains(asn) {
                    documentation_prefix()
                } else {
                    return Vec::new();
                }
            }
        };

        if wanted == documentation_prefix() {
            vec![
                BgpPath {
                    prefix: Some(wanted),
                    next_hop: Some(addr("192.0.2.254")),
                    peer: Some(addr("192.0.2.254")),
                    is_best: true,
                    is_valid: Some(true),
                    as_path: vec![65100, 65500],
                    local_pref: Some(150),
                    med: Some(10),
                    weight: Some(0),
                    origin: Some(Origin::Igp),
                    communities: vec![Community::parse("65001:100"), Community::parse("65100:500")],
                    rpki: RpkiValidation::from_router(RpkiStatus::Valid),
                },
                BgpPath {
                    prefix: Some(wanted),
                    next_hop: Some(addr("192.0.2.253")),
                    peer: Some(addr("192.0.2.253")),
                    is_best: false,
                    is_valid: Some(true),
                    as_path: vec![65200, 65300, 65500],
                    local_pref: Some(100),
                    // MED absent on this path on purpose: the interface has to
                    // render an unknown attribute as unknown.
                    med: None,
                    weight: Some(0),
                    origin: Some(Origin::Igp),
                    communities: vec![Community::parse("65001:200")],
                    rpki: RpkiValidation::from_router(RpkiStatus::Valid),
                },
                // A prepended path, with a 32-bit AS in it.
                //
                // Prepending is how an operator steers traffic and it is in
                // every real routing table. It was in none of this mock's,
                // and the interface crashed on the first real router it met:
                // the graph could not place an AS that appears three times,
                // and reported the failure as an unreachable router. The
                // fixtures the interface is developed against should look
                // like what it will be shown.
                BgpPath {
                    prefix: Some(wanted),
                    next_hop: Some(addr("192.0.2.251")),
                    peer: Some(addr("192.0.2.251")),
                    is_best: false,
                    is_valid: Some(true),
                    as_path: vec![65200, 65200, 65200, 65536, 65537, 65500],
                    local_pref: Some(100),
                    med: Some(200),
                    weight: Some(0),
                    origin: Some(Origin::Igp),
                    communities: vec![Community::parse("65001:300")],
                    rpki: RpkiValidation::from_router(RpkiStatus::Valid),
                },
            ]
        } else if wanted == aggregate_prefix() {
            // An aggregate carrying an AS_SET.
            //
            // The members are a set rather than a sequence, the model cannot
            // hold one yet, and the sequence therefore stops at the
            // aggregator — so the answer is partial and says so. Three
            // vendors print this and the interface had nothing to show it
            // with, because nothing produced it.
            vec![BgpPath {
                prefix: Some(wanted),
                next_hop: Some(addr("2001:db8::250")),
                peer: Some(addr("2001:db8::250")),
                is_best: true,
                is_valid: Some(true),
                as_path: vec![65300],
                local_pref: Some(100),
                med: Some(0),
                weight: Some(0),
                origin: Some(Origin::Igp),
                communities: vec![],
                rpki: RpkiValidation::from_router(RpkiStatus::NotFound),
            }]
        } else if wanted == hijacked_prefix() {
            vec![BgpPath {
                prefix: Some(wanted),
                next_hop: Some(addr("192.0.2.252")),
                peer: Some(addr("192.0.2.252")),
                is_best: true,
                is_valid: Some(true),
                as_path: vec![65400, 65534],
                local_pref: Some(100),
                med: Some(0),
                weight: Some(0),
                origin: Some(Origin::Incomplete),
                communities: vec![Community::parse("65535:666")],
                rpki: RpkiValidation::from_router(RpkiStatus::Invalid),
            }]
        } else if wanted == documentation_prefix_v6() {
            vec![BgpPath {
                prefix: Some(wanted),
                next_hop: Some(addr("2001:db8:ffff::1")),
                peer: Some(addr("2001:db8:ffff::1")),
                is_best: true,
                is_valid: Some(true),
                as_path: vec![65100, 65500],
                local_pref: Some(150),
                med: Some(10),
                weight: Some(0),
                origin: Some(Origin::Igp),
                communities: vec![Community::parse("65001:100:1")],
                // No ROA for this one: the third state the interface must show.
                rpki: RpkiValidation::from_router(RpkiStatus::NotFound),
            }]
        } else {
            Vec::new()
        }
    }

    /// The BGP result the mock would return for a target.
    pub fn bgp_route(&self, target: &QueryTarget) -> BgpRouteResult {
        let paths = self.paths_for(target);
        let raw = if paths.is_empty() {
            "% Network not in table\n".to_string()
        } else {
            format!(
                "Mock router — fabricated data for development and demonstration\n\
                 Paths: {}\n",
                paths.len()
            )
        };
        // The aggregate's path stops at an AS_SET the model cannot hold, so
        // the answer is partial — which the interface has a message for and
        // had nothing to produce it with.
        let completeness = if matches!(target, QueryTarget::Prefix(net) if *net == aggregate_prefix())
        {
            Completeness::Partial {
                unreadable_lines: 1,
            }
        } else {
            Completeness::Complete
        };

        BgpRouteResult {
            paths,
            raw_output: raw,
            completeness,
            truncated: false,
        }
    }

    /// A ping result for a target the mock knows, or a total loss for one it
    /// does not — which is what an unreachable address looks like.
    pub fn ping(&self, target: IpAddr, count: u8) -> PingResult {
        let reachable = [
            documentation_prefix(),
            hijacked_prefix(),
            documentation_prefix_v6(),
        ]
        .into_iter()
        .any(|net| net.contains(&target));

        let sent = u32::from(count.clamp(1, 20));
        if reachable {
            PingResult {
                packets_sent: sent,
                packets_received: sent,
                packet_loss_percent: 0.0,
                min_rtt_ms: Some(0.412),
                avg_rtt_ms: Some(0.501),
                max_rtt_ms: Some(0.688),
                raw_output: format!(
                    "Mock router — fabricated data\n\
                     PING {target}: {sent} packets, 0% loss, rtt min/avg/max = 0.412/0.501/0.688 ms\n"
                ),
            }
        } else {
            PingResult {
                packets_sent: sent,
                packets_received: 0,
                packet_loss_percent: 100.0,
                min_rtt_ms: None,
                avg_rtt_ms: None,
                max_rtt_ms: None,
                raw_output: format!(
                    "Mock router — fabricated data\n\
                     PING {target}: {sent} packets, 100% loss\n"
                ),
            }
        }
    }

    /// A short traceroute through the mock's transit path.
    pub fn traceroute(&self, target: IpAddr) -> TracerouteResult {
        let hops = vec![
            TracerouteHop {
                hop: 1,
                ip: Some("192.0.2.254".to_string()),
                hostname: Some("edge-01.mock.example".to_string()),
                rtt_ms: vec![0.412, 0.398, 0.431],
            },
            TracerouteHop {
                hop: 2,
                ip: Some("192.0.2.1".to_string()),
                hostname: Some("transit-a.mock.example".to_string()),
                rtt_ms: vec![1.204, 1.180, 1.233],
            },
            // A hop that does not answer: the interface has to render the gap
            // rather than hide it.
            TracerouteHop {
                hop: 3,
                ip: None,
                hostname: None,
                rtt_ms: Vec::new(),
            },
            TracerouteHop {
                hop: 4,
                ip: Some(target.to_string()),
                hostname: None,
                rtt_ms: vec![2.551, 2.498, 2.602],
            },
        ];

        TracerouteResult {
            target: target.to_string(),
            hops,
            raw_output: format!(
                "Mock router — fabricated data\ntraceroute to {target}, 4 hops max\n"
            ),
        }
    }
}

impl VendorDriver for MockDriver {
    fn vendor_name(&self) -> &'static str {
        MOCK_VENDOR
    }

    fn parse_ping(&self, _raw: &str) -> Result<PingResult, DriverError> {
        // The mock produces results directly; parsing is not part of its job,
        // and pretending otherwise would hide which code path a test exercised.
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "ping",
        })
    }

    fn parse_traceroute(&self, _raw: &str) -> Result<TracerouteResult, DriverError> {
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "traceroute",
        })
    }

    fn parse_bgp_route(&self, _raw: &str) -> Result<BgpRouteResult, DriverError> {
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "bgp_route",
        })
    }

    fn parse_bgp_summary(&self, _raw: &str) -> Result<BgpSummaryResult, DriverError> {
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "bgp_summary",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::parse_target;

    /// Documentation ranges only. If this fails, someone pasted a real capture
    /// into the fixtures.
    #[test]
    fn every_fixture_address_is_reserved_for_documentation() {
        let reserved: Vec<IpNet> = [
            "192.0.2.0/24",
            "198.51.100.0/24",
            "203.0.113.0/24",
            "2001:db8::/32",
        ]
        .iter()
        .map(|n| n.parse().unwrap())
        .collect();

        let targets = [
            parse_target("198.51.100.0/24").unwrap(),
            parse_target("203.0.113.0/24").unwrap(),
            parse_target("2001:db8:a::/48").unwrap(),
            parse_target("2001:db8::/32").unwrap(),
        ];

        for target in targets {
            for path in MockDriver.bgp_route(&target).paths {
                let prefix = path.prefix.expect("fixtures carry a prefix");
                assert!(
                    reserved.iter().any(|r| r.contains(&prefix)),
                    "prefix {prefix} is outside the documentation ranges"
                );
                for address in [path.next_hop, path.peer].into_iter().flatten() {
                    assert!(
                        reserved.iter().any(|r| r.contains(&address)),
                        "address {address} is outside the documentation ranges"
                    );
                }
                for asn in &path.as_path {
                    // Private (RFC 6996) or documentation (RFC 5398). The
                    // 32-bit documentation range is what lets the mock carry
                    // an AS above 65535, which several vendors print in a
                    // notation that drops it.
                    assert!(
                        (64512..=65534).contains(asn)
                            || (64496..=64511).contains(asn)
                            || (65536..=65551).contains(asn),
                        "AS{asn} is in neither a private nor a documentation range"
                    );
                }
            }
        }
    }

    #[test]
    fn answers_the_prefix_it_knows_with_a_best_path_and_an_alternative() {
        let result = MockDriver.bgp_route(&parse_target("198.51.100.0/24").unwrap());
        assert_eq!(result.paths.len(), 3);
        assert_eq!(result.completeness, Completeness::Complete);

        let best = result.best().expect("one path is marked best");
        assert_eq!(best.as_path, vec![65100, 65500]);
        assert_eq!(best.origin_as(), Some(65500));
        assert_eq!(best.rpki.status, RpkiStatus::Valid);

        // One path has no MED, so the interface has to render an unknown
        // attribute rather than a zero.
        assert!(result.paths.iter().any(|p| p.med.is_none()));
    }

    /// The interface crashed on the first real router it met, drawing a path
    /// with prepending, and could not have met one here: the mock's longest
    /// path was three ASNs and none repeated.
    #[test]
    fn a_path_is_prepended_and_carries_a_32_bit_as() {
        let result = MockDriver.bgp_route(&parse_target("198.51.100.0/24").unwrap());

        let prepended = result
            .paths
            .iter()
            .find(|p| p.as_path.len() > 3)
            .expect("one path is prepended");

        let first = prepended.as_path[0];
        assert!(
            prepended.as_path.iter().filter(|a| **a == first).count() >= 3,
            "the path repeats its first AS, the way an operator steers traffic"
        );
        assert!(
            prepended.as_path.iter().any(|asn| *asn > 65535),
            "and carries an AS above 65535, which some vendors print in asdot"
        );
    }

    /// An aggregate whose AS path stops at a set the model cannot hold. The
    /// interface has a message for a partial answer and nothing produced one.
    #[test]
    fn the_aggregate_is_partial_and_says_so() {
        let result = MockDriver.bgp_route(&parse_target("2001:db8:a::/48").unwrap());

        assert_eq!(result.paths.len(), 1);
        assert!(matches!(result.completeness, Completeness::Partial { .. }));
    }

    #[test]
    fn covers_the_rpki_states_the_interface_has_to_show() {
        let invalid = MockDriver.bgp_route(&parse_target("203.0.113.0/24").unwrap());
        assert_eq!(invalid.paths[0].rpki.status, RpkiStatus::Invalid);

        let not_found = MockDriver.bgp_route(&parse_target("2001:db8::/32").unwrap());
        assert_eq!(not_found.paths[0].rpki.status, RpkiStatus::NotFound);
    }

    #[test]
    fn an_unknown_prefix_is_a_real_no_route_answer() {
        let result = MockDriver.bgp_route(&parse_target("192.0.2.0/24").unwrap());
        assert!(result.paths.is_empty());
        assert_eq!(
            result.completeness,
            Completeness::Complete,
            "no route is an answer, not a parse failure"
        );
        assert!(result.raw_output.contains("not in table"));
    }

    #[test]
    fn ping_reports_loss_for_an_address_it_cannot_reach() {
        let reachable = MockDriver.ping(addr("198.51.100.10"), 5);
        assert_eq!(reachable.packets_received, 5);
        assert_eq!(reachable.packet_loss_percent, 0.0);

        let unreachable = MockDriver.ping(addr("192.0.2.10"), 5);
        assert_eq!(unreachable.packets_received, 0);
        assert_eq!(unreachable.packet_loss_percent, 100.0);
        assert_eq!(unreachable.avg_rtt_ms, None);
    }

    #[test]
    fn traceroute_includes_a_hop_that_does_not_answer() {
        let result = MockDriver.traceroute(addr("198.51.100.10"));
        assert_eq!(result.hops.len(), 4);
        let silent = &result.hops[2];
        assert_eq!(silent.ip, None);
        assert!(silent.rtt_ms.is_empty());
    }

    #[test]
    fn the_output_says_the_data_is_fabricated() {
        let result = MockDriver.bgp_route(&parse_target("198.51.100.0/24").unwrap());
        assert!(
            result.raw_output.contains("Mock router"),
            "an operator reading the raw output must see that it is not real"
        );
    }
}
