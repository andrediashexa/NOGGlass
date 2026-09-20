use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, DriverError, Origin,
    PingResult, TracerouteResult, VendorDriver,
};

pub struct CiscoDriver {
    pub is_xr: bool,
}

impl CiscoDriver {
    pub fn new(is_xr: bool) -> Self {
        Self { is_xr }
    }
}

impl VendorDriver for CiscoDriver {
    fn vendor_name(&self) -> &'static str {
        if self.is_xr {
            "cisco_iosxr"
        } else {
            "cisco_iosxe"
        }
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        // Exemplo Cisco:
        // Success rate is 100 percent (5/5), round-trip min/avg/max = 1/2/4 ms
        let mut sent = 0;
        let mut recv = 0;
        let mut loss = 100.0;
        let mut min = None;
        let mut avg = None;
        let mut max = None;

        for line in raw.lines() {
            if line.contains("Success rate is") {
                if let Some(rate_str) = line.split("percent").next() {
                    if let Some(rate) = rate_str
                        .split_whitespace()
                        .last()
                        .and_then(|s| s.parse::<f64>().ok())
                    {
                        loss = 100.0 - rate;
                    }
                }
                if let Some(counts) = line.split('(').nth(1).and_then(|s| s.split(')').next()) {
                    let parts: Vec<&str> = counts.split('/').collect();
                    if parts.len() == 2 {
                        recv = parts[0].parse().unwrap_or(0);
                        sent = parts[1].parse().unwrap_or(0);
                    }
                }
            } else if line.contains("round-trip min/avg/max") {
                if let Some(vals) = line.split('=').nth(1) {
                    let clean = vals.trim().replace("ms", "");
                    let parts: Vec<&str> = clean.split('/').collect();
                    if parts.len() >= 3 {
                        min = parts[0].trim().parse().ok();
                        avg = parts[1].trim().parse().ok();
                        max = parts[2].trim().parse().ok();
                    }
                }
            }
        }

        Ok(PingResult {
            packets_sent: sent,
            packets_received: recv,
            packet_loss_percent: loss,
            min_rtt_ms: min,
            avg_rtt_ms: avg,
            max_rtt_ms: max,
            raw_output: raw.to_string(),
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

    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // Tabular `show bgp` output, the form IOS-XE and IOS-XR share:
        //
        //    Network          Next Hop   Metric LocPrf Weight Path
        // *>i198.51.100.0/24  192.0.2.254    10    150      0 65100 65500 i
        //
        // The status column is glued to the network on IOS, so it is split off
        // by character rather than by whitespace.
        let mut paths = Vec::new();
        let mut unreadable = 0usize;
        let mut current_prefix = None;

        for line in raw.lines() {
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            let body = trimmed.trim_start();
            if body.starts_with("Network")
                || body.starts_with("BGP table")
                || body.starts_with("Status codes")
                || body.starts_with("Origin codes")
                || body.starts_with("Route Distinguisher")
                || body.starts_with("Processed")
            {
                continue;
            }
            if !body.starts_with('*') && !body.starts_with("r>") && !body.starts_with('r') {
                unreadable += 1;
                continue;
            }

            // Status flags are the leading non-alphanumeric characters plus the
            // internal/external marker.
            let flags: String = body
                .chars()
                .take_while(|c| matches!(c, '*' | '>' | 'r' | 's' | 'd' | 'h' | 'i' | 'S' | ' '))
                .collect();
            let rest = &body[flags.len()..];
            let mut fields = rest.split_whitespace();

            let Some(first) = fields.next() else {
                unreadable += 1;
                continue;
            };

            // A continuation line omits the network and starts at the next hop.
            let (prefix, next_hop_text) = match parse_network(first) {
                Some(net) => {
                    current_prefix = Some(net);
                    (Some(net), fields.next())
                }
                None => (current_prefix, Some(first)),
            };

            let remaining: Vec<&str> = fields.collect();
            // Metric, LocPrf and Weight are right-aligned numbers; the AS path
            // and the origin marker follow.
            let numbers: Vec<u32> = remaining
                .iter()
                .take_while(|t| t.bytes().all(|b| b.is_ascii_digit()))
                .filter_map(|t| t.parse().ok())
                .collect();
            let as_path: Vec<u32> = remaining
                .iter()
                .skip(numbers.len())
                .filter_map(|t| t.parse::<u32>().ok())
                .collect();
            let origin = remaining
                .last()
                .and_then(|t| t.chars().last())
                .and_then(Origin::from_marker);

            paths.push(BgpPath {
                prefix,
                next_hop: next_hop_text.and_then(parse_hop),
                is_best: flags.contains('>'),
                is_valid: Some(flags.contains('*')),
                as_path,
                // Positional: metric, then local preference, then weight.
                med: numbers.first().copied(),
                local_pref: numbers.get(1).copied(),
                weight: numbers.get(2).copied(),
                origin,
                ..BgpPath::default()
            });
        }

        Ok(BgpRouteResult::new(paths, raw).partial(unreadable))
    }

    fn parse_bgp_summary(&self, _raw: &str) -> Result<BgpSummaryResult, DriverError> {
        // An empty peer list would read as "this router has no BGP sessions".
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "bgp_summary",
        })
    }
}
