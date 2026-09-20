use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, Community, DriverError,
    Origin, PingResult, TracerouteResult, VendorDriver,
};

pub struct MikrotikDriver {
    pub is_v7: bool,
}

impl MikrotikDriver {
    pub fn new(is_v7: bool) -> Self {
        Self { is_v7 }
    }
}

impl VendorDriver for MikrotikDriver {
    fn vendor_name(&self) -> &'static str {
        // One name, matching the catalogue key. Returning a versioned name here
        // made errors quote a vendor the catalogue has never heard of, and the
        // version only changes how output is read, not which commands are sent.
        "mikrotik_routeros"
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        // RouterOS ends a ping with a summary of key=value pairs:
        //   sent=5 received=5 packet-loss=0% min-rtt=1ms234us avg-rtt=1ms456us max-rtt=2ms12us
        let mut sent = 0;
        let mut recv = 0;
        let mut loss = 100.0;
        let mut min = None;
        let mut avg = None;
        let mut max = None;

        for token in raw.split_whitespace() {
            if let Some(val) = token.strip_prefix("sent=") {
                sent = val.parse().unwrap_or(0);
            } else if let Some(val) = token.strip_prefix("received=") {
                recv = val.parse().unwrap_or(0);
            } else if let Some(val) = token.strip_prefix("packet-loss=") {
                loss = val.trim_end_matches('%').parse().unwrap_or(100.0);
            } else if let Some(val) = token.strip_prefix("avg-rtt=") {
                avg = parse_mikrotik_time(val);
            } else if let Some(val) = token.strip_prefix("min-rtt=") {
                min = parse_mikrotik_time(val);
            } else if let Some(val) = token.strip_prefix("max-rtt=") {
                max = parse_mikrotik_time(val);
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

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
    }

    /// Parser de Bloco Chave=Valor do RouterOS (/routing/bgp/route/print detail)
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // RouterOS prints `/routing/route/print detail` as numbered blocks:
        //
        // 0 ADb dst=198.51.100.0/24 gateway=192.0.2.254 as-path=65100,65500 ...
        //
        // The letters after the index are flags, and only those count as flags:
        // scanning the whole block for a letter matched values too, which made
        // almost every route look like the best one.
        let mut paths = Vec::new();

        for block in raw.split("\n\n") {
            let block = block.trim();
            if block.is_empty() || block.starts_with("Flags:") {
                continue;
            }

            let mut path = BgpPath::default();

            // Flags are the second whitespace-separated token, after the index.
            let mut head = block.split_whitespace();
            let flags = match (head.next(), head.next()) {
                (Some(index), Some(flags))
                    if index.bytes().all(|b| b.is_ascii_digit())
                        && flags.chars().all(|c| c.is_ascii_alphabetic()) =>
                {
                    flags
                }
                _ => "",
            };
            // A is active — the route the router is using. b only says the
            // route came from BGP, and every route in this output did, so
            // treating it as "best" marked all of them.
            path.is_best = flags.contains('A');
            // X is disabled; anything else is a route the router accepted.
            path.is_valid = Some(!flags.contains('X'));

            for token in block.split_whitespace() {
                // v6 prints `dst-address=`, v7 `dst-address=` too, and some
                // builds abbreviate to `dst=`.
                if let Some(val) = token
                    .strip_prefix("dst-address=")
                    .or_else(|| token.strip_prefix("dst="))
                {
                    path.prefix = parse_network(val);
                } else if let Some(val) = token
                    .strip_prefix("immediate-gw=")
                    .or_else(|| token.strip_prefix("gateway="))
                {
                    // `gateway=` can name an interface rather than an address;
                    // parse_hop returns None for that rather than inventing one.
                    path.next_hop = parse_hop(val.split('%').next().unwrap_or(val));
                } else if let Some(val) = token.strip_prefix("as-path=") {
                    path.as_path = val
                        .trim_matches('"')
                        .split([',', ' '])
                        .filter_map(|p| p.trim().parse::<u32>().ok())
                        .collect();
                } else if let Some(val) = token.strip_prefix("local-pref=") {
                    path.local_pref = val.parse().ok();
                } else if let Some(val) = token.strip_prefix("med=") {
                    path.med = val.parse().ok();
                } else if let Some(val) = token.strip_prefix("origin=") {
                    path.origin = match val.trim_matches('"').to_ascii_lowercase().as_str() {
                        "igp" => Some(Origin::Igp),
                        "egp" => Some(Origin::Egp),
                        "incomplete" => Some(Origin::Incomplete),
                        _ => None,
                    };
                } else if let Some(val) = token.strip_prefix("bgp-communities=") {
                    path.communities = val
                        .trim_matches('"')
                        .split(',')
                        .map(Community::parse)
                        .collect();
                }
            }

            if path.prefix.is_some() {
                paths.push(path);
            }
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

fn parse_mikrotik_time(s: &str) -> Option<f64> {
    // Converte formato MikroTik "1ms234us" para ms decimal
    if s.contains("ms") {
        let parts: Vec<&str> = s.split("ms").collect();
        let ms: f64 = parts[0].parse().unwrap_or(0.0);
        let us: f64 = parts
            .get(1)
            .and_then(|u| u.trim_end_matches("us").parse().ok())
            .unwrap_or(0.0);
        Some(ms + (us / 1000.0))
    } else {
        s.replace("us", "")
            .parse::<f64>()
            .ok()
            .map(|us| us / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{Completeness, QueryType, RpkiStatus};

    /// RouterOS 7 prints one block per route, as key=value pairs.
    #[test]
    fn parses_routeros_7_route_blocks() {
        let raw = "\
Flags: X - disabled, A - active, D - dynamic, b - bgp

 0 ADb dst-address=198.51.100.0/24 gateway=192.0.2.254 immediate-gw=192.0.2.254
       as-path=\"65100,65500\" local-pref=150 med=10 origin=igp
       bgp-communities=\"65001:100,65100:500\"

 1  Db dst-address=198.51.100.0/24 gateway=192.0.2.253
       as-path=\"65200,65300,65500\" local-pref=100 origin=igp
";
        let result = MikrotikDriver::new(true)
            .parse_bgp_route(raw)
            .expect("RouterOS output should parse");

        assert_eq!(result.paths.len(), 2);
        assert_eq!(result.completeness, Completeness::Complete);

        let best = result.best().expect("the A flag marks the active route");
        assert_eq!(best.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(best.next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(best.as_path, vec![65100, 65500]);
        assert_eq!(best.local_pref, Some(150));
        assert_eq!(best.med, Some(10));
        assert_eq!(best.origin, Some(Origin::Igp));
        assert_eq!(best.communities.len(), 2);
        assert_eq!(best.communities[0].raw, "65001:100");

        let alternative = &result.paths[1];
        assert!(
            !alternative.is_best,
            "only the route with the A flag is active"
        );
        assert_eq!(alternative.as_path, vec![65200, 65300, 65500]);
        assert_eq!(alternative.med, None, "this block has no med");
    }

    /// The flags are the token after the index. Scanning the whole block for a
    /// letter matched values as well, so nearly every route looked active.
    #[test]
    fn flags_are_read_from_the_flags_token_only() {
        let raw = " 0  Db dst-address=198.51.100.0/24 gateway=192.0.2.254 as-path=\"65100\" comment=\"backup A\"\n";
        let result = MikrotikDriver::new(true).parse_bgp_route(raw).unwrap();
        assert!(!result.paths[0].is_best, "the A in a comment is not a flag");
    }

    #[test]
    fn a_gateway_that_is_an_interface_is_not_a_next_hop() {
        let raw = " 0 ADb dst-address=198.51.100.0/24 gateway=ether1 as-path=\"65100\"\n";
        let result = MikrotikDriver::new(true).parse_bgp_route(raw).unwrap();
        assert_eq!(
            result.paths[0].next_hop, None,
            "an interface name is not an address, and must not be invented into one"
        );
        assert_eq!(result.paths[0].as_path, vec![65100]);
    }

    #[test]
    fn parses_the_ping_summary() {
        let raw = "\
  SEQ HOST                                     SIZE TTL TIME       STATUS
    0 198.51.100.1                               56  58 1ms234us
    1 198.51.100.1                               56  58 1ms456us
    sent=5 received=5 packet-loss=0% min-rtt=1ms234us avg-rtt=1ms456us max-rtt=2ms12us
";
        let result = MikrotikDriver::new(true).parse_ping(raw).unwrap();
        assert_eq!(result.packets_sent, 5);
        assert_eq!(result.packets_received, 5);
        assert_eq!(result.packet_loss_percent, 0.0);
        assert_eq!(result.min_rtt_ms, Some(1.234));
        assert_eq!(result.avg_rtt_ms, Some(1.456));
        assert_eq!(result.max_rtt_ms, Some(2.012));
    }

    #[test]
    fn a_ping_that_lost_everything_says_so() {
        let raw = "    sent=5 received=0 packet-loss=100%\n";
        let result = MikrotikDriver::new(true).parse_ping(raw).unwrap();
        assert_eq!(result.packets_received, 0);
        assert_eq!(result.packet_loss_percent, 100.0);
        assert_eq!(result.avg_rtt_ms, None, "there were no round trips to time");
    }

    /// RPKI is not reported by RouterOS, so it stays unchecked rather than
    /// being guessed — this is the vendor ADR-0010 exists for.
    #[test]
    fn rpki_state_is_left_unchecked() {
        let raw = " 0 ADb dst-address=198.51.100.0/24 gateway=192.0.2.254 as-path=\"65100\"\n";
        let result = MikrotikDriver::new(true).parse_bgp_route(raw).unwrap();
        assert_eq!(result.paths[0].rpki.status, RpkiStatus::NotChecked);
    }

    #[test]
    fn the_catalogue_covers_every_query_this_driver_offers() {
        let commands = crate::catalogue::BUILTIN
            .vendor("mikrotik_routeros")
            .unwrap();
        assert!(commands.ping_v4.is_some());
        assert!(commands.traceroute_v4.is_some());
        assert!(commands.bgp_route_v4.is_some());
        assert!(commands.bgp_summary.is_some());

        // RouterOS has no AS-path regex lookup, so an ASN query is refused
        // rather than approximated.
        assert!(commands.bgp_route_asn.is_none());

        // The driver name and the catalogue key have to match, or an error
        // quotes a vendor that does not exist.
        assert_eq!(MikrotikDriver::new(true).vendor_name(), "mikrotik_routeros");
        let _ = QueryType::BgpRoute;
    }
}
