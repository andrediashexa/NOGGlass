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
        // IOS and IOS-XR both end a ping with one summary line:
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
            }
            // The round-trip figures are on the same line as the success rate,
            // so this cannot be an `else`: it used to be, and every Cisco ping
            // came back without timings.
            if line.contains("round-trip min/avg/max") {
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

            // A continuation line omits the network and starts at the next
            // hop, and a next hop parses as a network too — as a host route —
            // so "does it parse" cannot decide. What decides: a network is
            // written in CIDR form, or it is followed by another address, which
            // is the next hop. Getting this wrong attributed a path to a /32 of
            // its own next hop.
            let second = fields.clone().next();
            let first_is_network =
                first.contains('/') || second.is_some_and(|token| parse_hop(token).is_some());

            let (prefix, next_hop_text) = if first_is_network {
                let parsed = parse_network(first);
                if parsed.is_some() {
                    current_prefix = parsed;
                }
                (current_prefix, fields.next())
            } else {
                (current_prefix, Some(first))
            };

            let remaining: Vec<&str> = fields.collect();

            // Metric, LocPrf and Weight are right-aligned numbers, then the AS
            // path, then the origin marker. Every one of those is a number, so
            // the split is positional: at most three metric columns, and the
            // rest is the path. Taking numbers greedily swallowed the AS path
            // and left it empty.
            let metric_columns = remaining
                .iter()
                .take(3)
                .take_while(|t| t.bytes().all(|b| b.is_ascii_digit()))
                .count();
            let numbers: Vec<u32> = remaining
                .iter()
                .take(metric_columns)
                .filter_map(|t| t.parse().ok())
                .collect();
            let as_path: Vec<u32> = remaining
                .iter()
                .skip(metric_columns)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Completeness;

    /// IOS-XE tabular output. The status flags are glued to the network, and
    /// the numeric columns are right-aligned, so a whitespace split reads them
    /// in the wrong places.
    #[test]
    fn parses_ios_xe_tables_with_attributes() {
        let raw = "\
BGP table version is 42, local router ID is 192.0.2.1
Status codes: s suppressed, d damped, h history, * valid, > best, i - internal
Origin codes: i - IGP, e - EGP, ? - incomplete

     Network          Next Hop            Metric LocPrf Weight Path
 *>  198.51.100.0/24  192.0.2.254             10    150      0 65100 65500 i
 *                   192.0.2.253             20    100      0 65200 65500 i
";
        let result = CiscoDriver::new(false)
            .parse_bgp_route(raw)
            .expect("IOS-XE output should parse");

        assert_eq!(result.completeness, Completeness::Complete);
        assert_eq!(result.paths.len(), 2);

        let best = result.best().expect("the > flag marks the best path");
        assert_eq!(best.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(best.next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(
            best.as_path,
            vec![65100, 65500],
            "the AS path was never populated before"
        );
        assert_eq!(best.med, Some(10));
        assert_eq!(best.local_pref, Some(150));
        assert_eq!(best.weight, Some(0));
        assert_eq!(best.origin, Some(Origin::Igp));
        assert_eq!(best.origin_as(), Some(65500));

        // The second line omits the network: it belongs to the prefix above.
        let alternative = &result.paths[1];
        assert_eq!(
            alternative.prefix.unwrap().to_string(),
            "198.51.100.0/24",
            "a continuation line keeps the prefix"
        );
        assert_eq!(alternative.next_hop.unwrap().to_string(), "192.0.2.253");
        assert!(!alternative.is_best);
    }

    /// IOS-XR prints the same table. The driver used to ask for JSON and then
    /// parse text, so every XR query came back empty.
    #[test]
    fn parses_ios_xr_tables() {
        let raw = "\
     Network          Next Hop            Metric LocPrf Weight Path
 *>i 198.51.100.0/24  192.0.2.254              0    150      0 65100 65500 i
";
        let result = CiscoDriver::new(true)
            .parse_bgp_route(raw)
            .expect("IOS-XR output should parse");

        assert_eq!(result.paths.len(), 1, "XR used to return nothing at all");
        assert_eq!(result.paths[0].as_path, vec![65100, 65500]);
        assert!(result.paths[0].is_best);
    }

    #[test]
    fn the_catalogue_no_longer_asks_cisco_for_json() {
        for vendor in ["cisco_iosxe", "cisco_iosxr"] {
            let commands = crate::catalogue::BUILTIN.vendor(vendor).unwrap();
            for template in [&commands.bgp_route_v4, &commands.bgp_route_v6] {
                let template = template.as_ref().expect("Cisco answers BGP queries");
                assert!(
                    !template.contains("json"),
                    "{vendor} asks for JSON but the driver parses text: {template}"
                );
            }
        }
    }

    #[test]
    fn a_table_with_no_routes_is_a_complete_answer() {
        let raw = "% Network not in table\n";
        let result = CiscoDriver::new(false).parse_bgp_route(raw).unwrap();
        assert!(result.paths.is_empty());
        assert_eq!(result.raw_output, raw);
    }

    #[test]
    fn ping_reports_what_the_router_said() {
        let raw = "\
Type escape sequence to abort.
Sending 5, 100-byte ICMP Echos to 198.51.100.1, timeout is 2 seconds:
!!!!!
Success rate is 100 percent (5/5), round-trip min/avg/max = 1/2/4 ms
";
        let result = CiscoDriver::new(false).parse_ping(raw).unwrap();
        assert_eq!(result.packets_sent, 5);
        assert_eq!(result.packets_received, 5);
        assert_eq!(result.packet_loss_percent, 0.0);
        assert_eq!(result.avg_rtt_ms, Some(2.0));
    }

    /// A failed ping must read as failed, not as an absent result.
    #[test]
    fn a_ping_that_lost_everything_says_so() {
        let raw = "Success rate is 0 percent (0/5)\n";
        let result = CiscoDriver::new(false).parse_ping(raw).unwrap();
        assert_eq!(result.packets_received, 0);
        assert_eq!(result.packet_loss_percent, 100.0);
        assert_eq!(result.avg_rtt_ms, None);
    }
}
