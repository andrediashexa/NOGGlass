use crate::driver::{BgpPath, BgpSummaryResult, DriverError, PingResult, QueryTarget, RpkiStatus, TracerouteResult, VendorDriver};
use std::net::IpAddr;

/// Driver para Huawei VRP (NE40E, NE8000, etc.)
pub struct HuaweiVrpDriver;

impl VendorDriver for HuaweiVrpDriver {
    fn vendor_name(&self) -> &'static str {
        "huawei_vrp"
    }

    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("screen-length 0 temporary")
    }

    fn prompt_pattern(&self) -> &'static str {
        r"<[\w\.\-]+>|\[[\w\.\-]+\]"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping -c {} {}", count.clamp(1, 20), target)
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("tracert {}", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        match target {
            QueryTarget::Ip(ip) => {
                if ip.is_ipv4() {
                    format!("display bgp routing-table {}", ip)
                } else {
                    format!("display bgp ipv6 routing-table {}", ip)
                }
            }
            QueryTarget::Prefix(net) => {
                if net.addr().is_ipv4() {
                    format!("display bgp routing-table {} {}", net.addr(), net.netmask())
                } else {
                    format!("display bgp ipv6 routing-table {}/{}", net.addr(), net.prefix_len())
                }
            }
            QueryTarget::Asn(asn) => format!("display bgp routing-table as-path-filter {}", asn),
        }
    }

    fn format_bgp_summary(&self) -> String {
        "display bgp peer".to_string()
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        // Exemplo Huawei:
        // 5 packet(s) transmitted, 5 packet(s) received, 0.00% packet loss
        // round-trip min/avg/max = 1/2/4 ms
        let mut sent = 0;
        let mut recv = 0;
        let mut loss = 100.0;
        let mut min = None;
        let mut avg = None;
        let mut max = None;

        for line in raw.lines() {
            if line.contains("transmitted") && line.contains("received") {
                let parts: Vec<&str> = line.split(',').collect();
                if let Some(p) = parts.get(0) {
                    if let Some(num) = p.split_whitespace().next() {
                        sent = num.parse().unwrap_or(0);
                    }
                }
                if let Some(p) = parts.get(1) {
                    if let Some(num) = p.split_whitespace().next() {
                        recv = num.parse().unwrap_or(0);
                    }
                }
                if let Some(p) = parts.get(2) {
                    let clean = p.replace("% packet loss", "").replace('%', "");
                    if let Some(num) = clean.split_whitespace().next() {
                        loss = num.parse().unwrap_or(100.0);
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

    /// Parser de Texto BGP do Huawei VRP (Extrai Best Path '*' e '>' + Next-Hop + MED + LocPrf + AS-Path)
    fn parse_bgp_route(&self, raw: &str) -> Result<Vec<BgpPath>, DriverError> {
        let mut paths = Vec::new();
        // Huawei display bgp routing-table formato:
        // Total Number of Routes: 2
        // BGP Local router ID is 192.0.2.1
        // Status codes: * - valid, > - best, d - damped...
        // Network            NextHop        MED        LocPrf    PrefVal Path/Ogn
        // *>  198.51.100.0/24  192.0.2.254    10         150       0       65100 65500i
        // *                    198.51.100.254 50         100       0       65200 65500i

        let mut current_network = String::new();

        for line in raw.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with("Total") || line.starts_with("BGP") || line.starts_with("Status") || line.starts_with("Network") {
                continue;
            }

            // Checar se a linha começa com status de rota (*, *>, etc.)
            if line.starts_with('*') || line.starts_with('>') || line.starts_with(" *") || line.starts_with(" *>") {
                let is_best = line.contains('>');
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 3 {
                    continue;
                }

                // Índice dinâmico se a coluna Network está presente nesta linha
                let mut idx = 1;

                if parts.len() > idx && (parts[idx].contains('/') || parts[idx].contains('.')) {
                    // Pode ser o Network ou pode ser o NextHop se o Network foi omitido
                    // Em BGP VRP, se a linha omitir o Network (segundo path do mesmo prefixo),
                    // o primeiro IP é diretamente o NextHop!
                    if parts[idx].contains('/') {
                        current_network = parts[idx].to_string();
                        idx += 1;
                    }
                }

                if parts.len() > idx {
                    let next_hop = parts[idx].to_string();
                    idx += 1;

                    let med = parts.get(idx).and_then(|v| v.parse().ok());
                    idx += 1;

                    let local_pref = parts.get(idx).and_then(|v| v.parse().ok());
                    idx += 1; // PrefVal
                    idx += 1;

                    // Extrair AS Path e Origin
                    let mut as_path = Vec::new();
                    let mut origin = "IGP".to_string();

                    for &token in &parts[idx.min(parts.len())..] {
                        if token.ends_with('i') || token == "i" {
                            origin = "IGP".to_string();
                            let clean = token.trim_end_matches('i');
                            if let Ok(asn) = clean.parse::<u32>() {
                                as_path.push(asn);
                            }
                        } else if token.ends_with('?') || token == "?" {
                            origin = "Incomplete".to_string();
                            let clean = token.trim_end_matches('?');
                            if let Ok(asn) = clean.parse::<u32>() {
                                as_path.push(asn);
                            }
                        } else if let Ok(asn) = token.parse::<u32>() {
                            as_path.push(asn);
                        }
                    }

                    paths.push(BgpPath {
                        is_best,
                        network: current_network.clone(),
                        next_hop,
                        as_path,
                        local_pref,
                        med,
                        weight: None,
                        origin,
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
