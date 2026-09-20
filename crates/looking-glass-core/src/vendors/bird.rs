use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, Community, DriverError,
    Origin, PingResult, TracerouteResult, VendorDriver,
};

pub struct BirdDriver;

impl VendorDriver for BirdDriver {
    fn vendor_name(&self) -> &'static str {
        "bird_routing_daemon"
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

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
    }

    /// Parser de Bloco BIRD (show route for <ip> all)
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // BIRD 2 prints one route as a header line plus indented attributes:
        //
        // 198.51.100.0/24 unicast [upstream1 12:00:00] * (100) [AS65500i]
        //     via 192.0.2.254 on eth0
        //     BGP.as_path: 65100 65500
        //     BGP.local_pref: 150
        //     BGP.community: (65001,100) (65100,500)
        //
        // Every attribute belongs to the header above it, so the accumulator is
        // replaced wholesale on each header. Clearing only some fields used to
        // leak the previous route's next hop and metrics into the next one.
        let mut paths: Vec<BgpPath> = Vec::new();
        let mut current: Option<BgpPath> = None;

        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if line.contains("unicast") || line.contains("unreachable") {
                if let Some(path) = current.take() {
                    paths.push(path);
                }
                let mut path = BgpPath {
                    is_best: line.contains('*'),
                    ..BgpPath::default()
                };
                if let Some(prefix) = line.split_whitespace().next() {
                    path.prefix = parse_network(prefix);
                }
                // The trailing [AS65500i] carries the origin marker.
                if let Some(tail) = line.rsplit('[').next() {
                    let tail = tail.trim_end_matches(']');
                    if let Some(marker) = tail.chars().last() {
                        path.origin = Origin::from_marker(marker);
                    }
                }
                current = Some(path);
                continue;
            }

            let Some(path) = current.as_mut() else {
                continue;
            };

            if let Some(rest) = line.strip_prefix("via ") {
                path.next_hop = rest.split_whitespace().next().and_then(parse_hop);
            } else if let Some(rest) = line.strip_prefix("BGP.as_path:") {
                path.as_path = rest
                    .split_whitespace()
                    .filter_map(|token| token.parse::<u32>().ok())
                    .collect();
            } else if let Some(rest) = line.strip_prefix("BGP.local_pref:") {
                path.local_pref = rest.trim().parse().ok();
            } else if let Some(rest) = line.strip_prefix("BGP.med:") {
                path.med = rest.trim().parse().ok();
            } else if let Some(rest) = line.strip_prefix("BGP.community:") {
                path.communities = rest.split_whitespace().map(Community::parse).collect();
            } else if let Some(rest) = line.strip_prefix("BGP.large_community:") {
                path.communities
                    .extend(rest.split_whitespace().map(Community::parse));
            } else if let Some(rest) = line.strip_prefix("BGP.next_hop:") {
                path.next_hop = rest.split_whitespace().next().and_then(parse_hop);
            }
        }

        if let Some(path) = current.take() {
            paths.push(path);
        }

        Ok(BgpRouteResult::new(paths, raw))
    }

    fn parse_bgp_summary(&self, _raw: &str) -> Result<BgpSummaryResult, DriverError> {
        // An empty peer list would read as "this router has no BGP sessions".
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "bgp_summary",
        })
    }
}
