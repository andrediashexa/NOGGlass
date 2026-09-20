use crate::driver::{
    BgpPath, BgpSummaryResult, DriverError, PingResult, QueryTarget, RpkiStatus, TracerouteResult,
    VendorDriver,
};
use std::net::IpAddr;

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

    fn disable_paging_cmd(&self) -> Option<&'static str> {
        None
    }

    fn prompt_pattern(&self) -> &'static str {
        r"\[[\w\.\-]+@[\w\.\-]+\]\s*[>#]"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("/ping address={} count={}", target, count.clamp(1, 20))
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("/tool traceroute address={} count=1 use-dns=no", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        if self.is_v7 {
            match target {
                QueryTarget::Ip(ip) => format!("/routing/bgp/route/print detail where dst={}", ip),
                QueryTarget::Prefix(net) => {
                    format!("/routing/bgp/route/print detail where dst={}", net)
                }
                QueryTarget::Asn(asn) => {
                    format!("/routing/bgp/route/print detail where as-path~\"{}\"", asn)
                }
            }
        } else {
            match target {
                QueryTarget::Ip(ip) => format!(
                    "/routing bgp advertisements print detail where dst-address={}",
                    ip
                ),
                QueryTarget::Prefix(net) => format!(
                    "/routing bgp advertisements print detail where dst-address={}",
                    net
                ),
                QueryTarget::Asn(asn) => format!(
                    "/routing bgp advertisements print detail where as-path~\"{}\"",
                    asn
                ),
            }
        }
    }

    fn format_bgp_summary(&self) -> String {
        if self.is_v7 {
            "/routing/bgp/session/print detail".to_string()
        } else {
            "/routing bgp peer print detail".to_string()
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
    fn parse_bgp_route(&self, raw: &str) -> Result<Vec<BgpPath>, DriverError> {
        let mut paths = Vec::new();
        // Blocos começam com números ou flags: "Flags: X - active, D - dynamic, ..."
        // 0 ADb dst=198.51.100.0/24 gateway=192.0.2.254 as-path=65100,65500 local-pref=150 ...
        for block in raw.split("\n\n") {
            if block.trim().is_empty() || block.contains("Flags:") {
                continue;
            }

            let mut network = String::new();
            let mut next_hop = String::new();
            let mut as_path = Vec::new();
            let mut local_pref = None;
            let mut med = None;
            let is_best = block.contains("active") || block.contains('b') || block.contains('A');

            for token in block.split_whitespace() {
                if let Some(val) = token.strip_prefix("dst=") {
                    network = val.to_string();
                } else if let Some(val) = token.strip_prefix("gateway=") {
                    next_hop = val.to_string();
                } else if let Some(val) = token.strip_prefix("as-path=") {
                    for part in val.split(',') {
                        if let Ok(asn) = part.trim().parse::<u32>() {
                            as_path.push(asn);
                        }
                    }
                } else if let Some(val) = token.strip_prefix("local-pref=") {
                    local_pref = val.parse().ok();
                } else if let Some(val) = token.strip_prefix("med=") {
                    med = val.parse().ok();
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
                    communities: Vec::new(),
                    rpki_status: RpkiStatus::NotChecked,
                });
            }
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
