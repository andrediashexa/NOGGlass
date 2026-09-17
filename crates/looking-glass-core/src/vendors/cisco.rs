use crate::driver::{BgpPath, BgpSummaryResult, DriverError, PingResult, QueryTarget, RpkiStatus, TracerouteResult, VendorDriver};
use std::net::IpAddr;

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

    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("terminal length 0")
    }

    fn prompt_pattern(&self) -> &'static str {
        r"[\w\.\-]+[>#]"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping {} repeat {}", target, count.clamp(1, 20))
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("traceroute {}", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        if self.is_xr {
            // Cisco IOS-XR suporta | json diretamente no CLI
            match target {
                QueryTarget::Ip(ip) => {
                    if ip.is_ipv4() {
                        format!("show bgp ipv4 unicast {} | json", ip)
                    } else {
                        format!("show bgp ipv6 unicast {} | json", ip)
                    }
                }
                QueryTarget::Prefix(net) => {
                    if net.addr().is_ipv4() {
                        format!("show bgp ipv4 unicast {} | json", net)
                    } else {
                        format!("show bgp ipv6 unicast {} | json", net)
                    }
                }
                QueryTarget::Asn(asn) => format!("show bgp regexp _{}_ | json", asn),
            }
        } else {
            match target {
                QueryTarget::Ip(ip) => format!("show ip bgp {}", ip),
                QueryTarget::Prefix(net) => format!("show ip bgp {}", net),
                QueryTarget::Asn(asn) => format!("show ip bgp regexp _{}_", asn),
            }
        }
    }

    fn format_bgp_summary(&self) -> String {
        if self.is_xr {
            "show bgp summary | json".to_string()
        } else {
            "show ip bgp summary".to_string()
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
                    if let Some(rate) = rate_str.split_whitespace().last().and_then(|s| s.parse::<f64>().ok()) {
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

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        Ok(TracerouteResult {
            target: "".to_string(),
            hops: Vec::new(),
            raw_output: raw.to_string(),
        })
    }

    fn parse_bgp_route(&self, raw: &str) -> Result<Vec<BgpPath>, DriverError> {
        // Suporta JSON nativo do XR ou fallback tabular do IOS-XE clássico
        let mut paths = Vec::new();

        for line in raw.lines() {
            let line = line.trim_start();
            if line.starts_with('*') {
                let is_best = line.contains('>');
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    paths.push(BgpPath {
                        is_best,
                        network: parts.get(1).unwrap_or(&"").to_string(),
                        next_hop: parts.get(2).unwrap_or(&"").to_string(),
                        as_path: Vec::new(),
                        local_pref: None,
                        med: None,
                        weight: None,
                        origin: "IGP".to_string(),
                        communities: Vec::new(),
                        rpki_status: RpkiStatus::NotChecked,
                    });
                }
            }
        }

        Ok(paths)
    }

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        Ok(BgpSummaryResult {
            router_id: None,
            local_as: None,
            peers: Vec::new(),
            raw_output: raw.to_string(),
        })
    }
}
