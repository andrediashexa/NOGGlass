use crate::driver::{
    BgpPath, BgpSummaryResult, DriverError, PingResult, QueryTarget, RpkiStatus, TracerouteResult,
    VendorDriver,
};
use std::net::IpAddr;

pub struct BirdDriver;

impl VendorDriver for BirdDriver {
    fn vendor_name(&self) -> &'static str {
        "bird_routing_daemon"
    }

    fn disable_paging_cmd(&self) -> Option<&'static str> {
        None
    }

    fn prompt_pattern(&self) -> &'static str {
        r"bird>\s*$"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping -c {} {}", count.clamp(1, 20), target)
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("traceroute -n {}", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        match target {
            QueryTarget::Ip(ip) => format!("show route for {} all", ip),
            QueryTarget::Prefix(net) => format!("show route for {} all", net),
            QueryTarget::Asn(asn) => format!("show route where bgp_path ~ [= * {} * =] all", asn),
        }
    }

    fn format_bgp_summary(&self) -> String {
        "show protocols all".to_string()
    }

    fn parse_ping(&self, _raw: &str) -> Result<PingResult, DriverError> {
        // Previously returned a hardcoded 5/5 with 0% loss for any input, so a
        // fully failing ping rendered as a perfect one. Until a real parser
        // exists, say so.
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "ping",
        })
    }

    fn parse_traceroute(&self, _raw: &str) -> Result<TracerouteResult, DriverError> {
        // No hop parser yet. An empty hop list would be indistinguishable from
        // a traceroute that legitimately returned nothing.
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "traceroute",
        })
    }

    /// Parser de Bloco BIRD (show route for <ip> all)
    fn parse_bgp_route(&self, raw: &str) -> Result<Vec<BgpPath>, DriverError> {
        let mut paths = Vec::new();
        // BIRD 2 formato:
        // 198.51.100.0/24 unicast [upstream1 12:00:00] * (100) [AS65500i]
        //     via 192.0.2.254 on eth0
        //     Type: BGP univ
        //     BGP.as_path: 65100 65500
        //     BGP.local_pref: 150
        //     BGP.community: (65001,100) (65100,500)
        let mut is_best = false;
        let mut network = String::new();
        let mut next_hop = String::new();
        let mut as_path = Vec::new();
        let mut local_pref = None;
        let mut med = None;
        let mut communities = Vec::new();

        for line in raw.lines() {
            let line = line.trim();
            if line.contains("unicast") {
                if !network.is_empty() {
                    paths.push(BgpPath {
                        is_best,
                        network: network.clone(),
                        next_hop: next_hop.clone(),
                        as_path: as_path.clone(),
                        local_pref,
                        med,
                        weight: None,
                        origin: None, // not parsed from this vendor output yet
                        communities: communities.clone(),
                        rpki_status: RpkiStatus::NotChecked,
                    });
                    as_path.clear();
                    communities.clear();
                }

                if let Some(prefix) = line.split_whitespace().next() {
                    network = prefix.to_string();
                }
                is_best = line.contains('*');
            } else if line.starts_with("via ") {
                if let Some(ip) = line.split_whitespace().nth(1) {
                    next_hop = ip.to_string();
                }
            } else if line.starts_with("BGP.as_path:") {
                for token in line.trim_start_matches("BGP.as_path:").split_whitespace() {
                    if let Ok(asn) = token.parse::<u32>() {
                        as_path.push(asn);
                    }
                }
            } else if line.starts_with("BGP.local_pref:") {
                local_pref = line
                    .trim_start_matches("BGP.local_pref:")
                    .trim()
                    .parse()
                    .ok();
            } else if line.starts_with("BGP.med:") {
                med = line.trim_start_matches("BGP.med:").trim().parse().ok();
            } else if line.starts_with("BGP.community:") {
                for comm in line.trim_start_matches("BGP.community:").split_whitespace() {
                    communities.push(comm.to_string());
                }
            }
        }

        if !network.is_empty() {
            paths.push(BgpPath {
                is_best,
                network,
                next_hop,
                as_path,
                local_pref,
                med,
                weight: None,
                origin: None, // not parsed from this vendor output yet
                communities,
                rpki_status: RpkiStatus::NotChecked,
            });
        }

        Ok(paths)
    }

    fn parse_bgp_summary(&self, _raw: &str) -> Result<BgpSummaryResult, DriverError> {
        // An empty peer list would read as "this router has no BGP sessions".
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "bgp_summary",
        })
    }
}
