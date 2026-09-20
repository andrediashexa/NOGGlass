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
        if self.is_v7 {
            "mikrotik_routeros_v7"
        } else {
            "mikrotik_routeros_v6"
        }
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        // Exemplo MikroTik:
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

    fn parse_traceroute(&self, _raw: &str) -> Result<TracerouteResult, DriverError> {
        // No hop parser yet. An empty hop list would be indistinguishable from
        // a traceroute that legitimately returned nothing.
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "traceroute",
        })
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
            path.is_best = flags.contains('A') || flags.contains('b');
            path.is_valid = Some(!flags.contains('I'));

            for token in block.split_whitespace() {
                if let Some(val) = token.strip_prefix("dst=") {
                    path.prefix = parse_network(val);
                } else if let Some(val) = token.strip_prefix("gateway=") {
                    path.next_hop = parse_hop(val);
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
