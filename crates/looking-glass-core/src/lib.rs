pub mod driver;
pub mod vendors;

pub use driver::{
    BgpPath, BgpPeerSummary, BgpSummaryResult, DriverError, PingResult, QueryTarget, QueryType,
    RpkiStatus, TracerouteHop, TracerouteResult, VendorDriver,
};
pub use vendors::*;

use regex::Regex;
use std::sync::LazyLock;

/// Regex universal de detecção de término de comando portado do Netmiko
pub static DEFAULT_BASE_PROMPT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\>#\]]\s*$").expect("Regex inválido de prompt padrão")
});

#[cfg(test)]
mod tests {
    use super::*;
    use ipnet::IpNet;
    use std::net::IpAddr;
    use std::str::FromStr;

    #[test]
    fn test_huawei_commands_and_parsing() {
        let driver = HuaweiVrpDriver;
        assert_eq!(driver.disable_paging_cmd(), Some("screen-length 0 temporary"));
        assert_eq!(driver.format_bgp_summary(), "display bgp peer");

        let raw_bgp = r#"
 Total Number of Routes: 2
 BGP Local router ID is 192.0.2.1
 Status codes: * - valid, > - best, d - damped
   Network            NextHop        MED        LocPrf    PrefVal Path/Ogn
*>  198.51.100.0/24    192.0.2.254    10         150       0       65100 65500i
*                      198.51.100.254 50         100       0       65200 65500i
"#;
        let paths = driver.parse_bgp_route(raw_bgp).expect("Falha no parse do Huawei VRP");
        assert_eq!(paths.len(), 2);
        assert!(paths[0].is_best);
        assert_eq!(paths[0].network, "198.51.100.0/24");
        assert_eq!(paths[0].next_hop, "192.0.2.254");
        assert_eq!(paths[0].as_path, vec![65100, 65500]);
        assert_eq!(paths[0].local_pref, Some(150));
        assert_eq!(paths[0].med, Some(10));

        assert!(!paths[1].is_best);
        assert_eq!(paths[1].next_hop, "198.51.100.254");
        assert_eq!(paths[1].as_path, vec![65200, 65500]);
    }

    #[test]
    fn test_juniper_json_parsing_with_rpki() {
        let driver = JuniperDriver;
        assert_eq!(driver.format_bgp_summary(), "show bgp summary | display json");

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

        let paths = driver.parse_bgp_route(raw_json).expect("Falha ao parsear JSON nativo do JunOS");
        assert_eq!(paths.len(), 1);
        let p = &paths[0];
        assert!(p.is_best);
        assert_eq!(p.network, "198.51.100.0/24");
        assert_eq!(p.next_hop, "192.0.2.254");
        assert_eq!(p.as_path, vec![65100, 65500]);
        assert_eq!(p.local_pref, Some(150));
        assert_eq!(p.rpki_status, RpkiStatus::Valid);
        assert_eq!(p.communities, vec!["65001:100", "65100:500"]);
    }

    #[test]
    fn test_mikrotik_parsing() {
        let driver = MikrotikDriver::new(true);
        let raw = "0 ADb dst=198.51.100.0/24 gateway=192.0.2.254 as-path=65100,65500 local-pref=150 med=10";
        let paths = driver.parse_bgp_route(raw).expect("Falha no parse MikroTik");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].network, "198.51.100.0/24");
        assert_eq!(paths[0].next_hop, "192.0.2.254");
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
        let paths = driver.parse_bgp_route(raw).expect("Falha no parse BIRD");
        assert_eq!(paths.len(), 1);
        assert!(paths[0].is_best);
        assert_eq!(paths[0].network, "198.51.100.0/24");
        assert_eq!(paths[0].next_hop, "192.0.2.254");
        assert_eq!(paths[0].as_path, vec![65100, 65500]);
        assert_eq!(paths[0].local_pref, Some(150));
        assert_eq!(paths[0].communities, vec!["(65001,100)", "(65100,500)"]);
    }
}
