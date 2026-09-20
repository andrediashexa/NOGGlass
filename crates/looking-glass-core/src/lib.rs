pub mod catalogue;
pub mod driver;
pub mod target;
pub mod vendors;

pub use catalogue::{Catalogue, CatalogueError, BUILTIN};
pub use driver::{
    BgpPath, BgpPeerSummary, BgpRouteResult, BgpSummaryResult, Community, CommunityKind,
    Completeness, DriverError, Origin, PingResult, QueryTarget, QueryType, RpkiSource, RpkiStatus,
    RpkiValidation, TracerouteHop, TracerouteResult, VendorDriver,
};
pub use target::{parse_target, QueryLimits, TargetError, MAX_TARGET_LEN};
pub use vendors::*;

use regex::Regex;
use std::sync::LazyLock;

/// Generic end-of-output prompt detection, ported from Netmiko.
pub static DEFAULT_BASE_PROMPT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[>#\]]\s*$").expect("invalid default prompt regex"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huawei_commands_and_parsing() {
        let driver = HuaweiVrpDriver;
        // Commands now come from the catalogue, not from the driver.
        assert_eq!(
            BUILTIN
                .vendor("huawei_vrp")
                .unwrap()
                .disable_paging
                .as_deref(),
            Some("screen-length 0 temporary")
        );
        assert_eq!(
            BUILTIN.bgp_summary("huawei_vrp").unwrap(),
            "display bgp peer"
        );

        let raw_bgp = r#"
 Total Number of Routes: 2
 BGP Local router ID is 192.0.2.1
 Status codes: * - valid, > - best, d - damped
   Network            NextHop        MED        LocPrf    PrefVal Path/Ogn
*>  198.51.100.0/24    192.0.2.254    10         150       0       65100 65500i
*                      198.51.100.254 50         100       0       65200 65500i
"#;
        let result = driver
            .parse_bgp_route(raw_bgp)
            .expect("Huawei VRP output should parse");
        assert_eq!(
            result.completeness,
            Completeness::Complete,
            "every line of this fixture is a route or a header"
        );
        assert_eq!(
            result.raw_output, raw_bgp,
            "raw output must always be preserved"
        );
        let paths = &result.paths;
        assert_eq!(paths.len(), 2);
        assert!(paths[0].is_best);
        assert_eq!(paths[0].prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(paths[0].next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(paths[0].as_path, vec![65100, 65500]);
        assert_eq!(paths[0].local_pref, Some(150));
        assert_eq!(paths[0].med, Some(10));

        assert!(!paths[1].is_best);
        assert_eq!(paths[1].next_hop.unwrap().to_string(), "198.51.100.254");
        assert_eq!(paths[1].as_path, vec![65200, 65500]);
    }

    #[test]
    fn test_juniper_json_parsing_with_rpki() {
        let driver = JuniperDriver;
        assert_eq!(
            BUILTIN.bgp_summary("juniper_junos").unwrap(),
            "show bgp summary | display json"
        );

        let raw_json = r#"{
            "route-information": [{
                "route-table": [{
                    "table-name": [{"data": "inet.0"}],
                    "rt": [{
                        "rt-destination": [{"data": "198.51.100.0/24"}],
                        "rt-entry": [{
                            "active-tag": [{"data": "*"}],
                            "validation-state": [{"data": "valid"}],
                            "as-path": [{"data": "65100 65500 I"}],
                            "local-preference": [{"data": "150"}],
                            "metric": [{"data": "10"}],
                            "nh": [{"to": [{"data": "192.0.2.254"}]}],
                            "communities": [{"community": [{"data": "65001:100"}, {"data": "65100:500"}]}]
                        }]
                    }]
                }]
            }]
        }"#;

        let result = driver
            .parse_bgp_route(raw_json)
            .expect("Junos JSON should parse");
        let paths = &result.paths;
        assert_eq!(paths.len(), 1);
        let p = &paths[0];
        assert!(p.is_best);
        assert_eq!(p.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(p.next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(p.as_path, vec![65100, 65500]);
        assert_eq!(p.local_pref, Some(150));
        assert_eq!(p.rpki.status, RpkiStatus::Valid);
        assert_eq!(
            p.rpki.source,
            RpkiSource::Router,
            "Junos reported it, so the source is the router"
        );
        let raw_communities: Vec<&str> = p.communities.iter().map(|c| c.raw.as_str()).collect();
        assert_eq!(raw_communities, vec!["65001:100", "65100:500"]);
        assert!(p
            .communities
            .iter()
            .all(|c| c.kind == CommunityKind::Standard));
    }

    #[test]
    fn test_mikrotik_parsing() {
        let driver = MikrotikDriver::new(true);
        let raw = "0 ADb dst=198.51.100.0/24 gateway=192.0.2.254 as-path=65100,65500 local-pref=150 med=10";
        let result = driver
            .parse_bgp_route(raw)
            .expect("RouterOS output should parse");
        let paths = &result.paths;
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(paths[0].next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(paths[0].as_path, vec![65100, 65500]);
        assert_eq!(paths[0].local_pref, Some(150));
    }

    #[test]
    fn test_bird_parsing() {
        let driver = BirdDriver;
        let raw = r#"
198.51.100.0/24 unicast [upstream1 12:00:00] * (100) [AS65500i]
    via 192.0.2.254 on eth0
    Type: BGP univ
    BGP.as_path: 65100 65500
    BGP.local_pref: 150
    BGP.community: (65001,100) (65100,500)
"#;
        let result = driver
            .parse_bgp_route(raw)
            .expect("BIRD output should parse");
        let paths = &result.paths;
        assert_eq!(paths.len(), 1);
        assert!(paths[0].is_best);
        assert_eq!(paths[0].prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(paths[0].next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(paths[0].as_path, vec![65100, 65500]);
        assert_eq!(paths[0].local_pref, Some(150));
        let raw_communities: Vec<&str> = paths[0]
            .communities
            .iter()
            .map(|c| c.raw.as_str())
            .collect();
        assert_eq!(raw_communities, vec!["(65001,100)", "(65100,500)"]);
        assert_eq!(paths[0].origin, Some(Origin::Igp), "BIRD prints [AS65500i]");
    }

    /// A driver that cannot parse something MUST say so. Returning a default
    /// value — a perfect ping, an empty hop list, an empty route list — makes a
    /// diagnostic tool lie to the operator reading it (ADR-0006).
    #[test]
    fn unimplemented_parsers_error_instead_of_inventing_results() {
        let ping_raw = "5 packets transmitted, 0 packets received, 100% packet loss";

        for (vendor, result) in [
            ("nokia_sros", NokiaSrosDriver.parse_ping(ping_raw)),
            ("datacom_dmos", DatacomDriver.parse_ping(ping_raw)),
            ("bird_routing_daemon", BirdDriver.parse_ping(ping_raw)),
        ] {
            match result {
                Err(DriverError::Unsupported { vendor: v, query }) => {
                    assert_eq!(v, vendor);
                    assert_eq!(query, "ping");
                }
                Err(other) => panic!("{vendor}: unexpected error {other}"),
                Ok(parsed) => {
                    panic!("{vendor}: invented a ping result from a 100% loss output: {parsed:?}")
                }
            }
        }
    }

    #[test]
    fn traceroute_is_not_silently_empty() {
        let raw = " 1  192.0.2.254  0.512 ms  0.480 ms  0.501 ms";
        let drivers: Vec<Box<dyn VendorDriver>> = vec![
            Box::new(HuaweiVrpDriver),
            Box::new(CiscoDriver::new(false)),
            Box::new(JuniperDriver),
            Box::new(MikrotikDriver::new(true)),
            Box::new(NokiaSrosDriver),
            Box::new(DatacomDriver),
            Box::new(BirdDriver),
        ];

        for driver in drivers {
            match driver.parse_traceroute(raw) {
                Err(DriverError::Unsupported { query, .. }) => assert_eq!(query, "traceroute"),
                Err(other) => panic!("{}: unexpected error {other}", driver.vendor_name()),
                Ok(result) => panic!(
                    "{}: returned {} hops from an unimplemented parser",
                    driver.vendor_name(),
                    result.hops.len()
                ),
            }
        }
    }

    /// VRP marks the origin with a trailing i, e or ? on the AS path. When the
    /// marker is absent the router did not report it, and the field stays None.
    #[test]
    fn huawei_origin_is_none_when_the_router_does_not_report_it() {
        let driver = HuaweiVrpDriver;
        let with_marker = r#"
   Network            NextHop        MED        LocPrf    PrefVal Path/Ogn
*>  198.51.100.0/24    192.0.2.254    10         150       0       65100 65500i
"#;
        let paths = driver
            .parse_bgp_route(with_marker)
            .expect("VRP output with an origin marker should parse")
            .paths;
        assert_eq!(paths[0].origin, Some(Origin::Igp));

        let without_marker = r#"
   Network            NextHop        MED        LocPrf    PrefVal Path/Ogn
*>  198.51.100.0/24    192.0.2.254    10         150       0       65100 65500
"#;
        let paths = driver
            .parse_bgp_route(without_marker)
            .expect("VRP output without an origin marker should still parse")
            .paths;
        assert_eq!(
            paths[0].origin, None,
            "origin must not be assumed when the router does not report it"
        );
    }

    /// ADR-0006: an empty path list means "no route", and MUST be
    /// distinguishable from "the parser could not read this".
    #[test]
    fn empty_result_is_not_the_same_as_a_parse_failure() {
        let driver = HuaweiVrpDriver;

        let no_route = " Total Number of Routes: 0\n";
        let result = driver
            .parse_bgp_route(no_route)
            .expect("a table with no routes is a valid answer");
        assert!(result.paths.is_empty());
        assert_eq!(result.completeness, Completeness::Complete);

        let garbled = "*>  this line is not a route at all\n";
        let result = driver
            .parse_bgp_route(garbled)
            .expect("unreadable output still returns the raw text");
        assert!(result.paths.is_empty());
        assert!(
            matches!(result.completeness, Completeness::Partial { .. }),
            "unreadable lines must be reported, got {:?}",
            result.completeness
        );
        assert_eq!(result.raw_output, garbled);
    }

    #[test]
    fn origin_as_is_the_last_hop_of_the_as_path() {
        let path = BgpPath {
            as_path: vec![65100, 65200, 65500],
            ..BgpPath::default()
        };
        assert_eq!(path.origin_as(), Some(65500));

        // A locally originated route has an empty AS path and therefore no
        // origin AS to report.
        assert_eq!(BgpPath::default().origin_as(), None);
    }

    #[test]
    fn communities_are_classified_without_inventing_meaning() {
        assert_eq!(Community::parse("65001:100").kind, CommunityKind::Standard);
        assert_eq!(Community::parse("65001:100:200").kind, CommunityKind::Large);
        assert_eq!(
            Community::parse("target:65001:100").kind,
            CommunityKind::Extended
        );

        let well_known = Community::parse("65535:666");
        assert_eq!(well_known.kind, CommunityKind::WellKnown);
        assert_eq!(well_known.name.as_deref(), Some("blackhole"));

        // An operator-defined value carries no name: we do not know what their
        // policy assigns to it, and guessing would mislead.
        assert_eq!(Community::parse("65001:100").name, None);
    }
}
