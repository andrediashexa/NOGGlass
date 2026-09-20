use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, DriverError, Origin,
    PingResult, RpkiValidation, TracerouteResult, VendorDriver,
};

/// Driver para Huawei VRP (NE40E, NE8000, etc.)
pub struct HuaweiVrpDriver;

impl VendorDriver for HuaweiVrpDriver {
    fn vendor_name(&self) -> &'static str {
        "huawei_vrp"
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
                if let Some(p) = parts.first() {
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

    fn parse_traceroute(&self, _raw: &str) -> Result<TracerouteResult, DriverError> {
        // No hop parser yet. An empty hop list would be indistinguishable from
        // a traceroute that legitimately returned nothing.
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "traceroute",
        })
    }

    /// Parser de Texto BGP do Huawei VRP (Extrai Best Path '*' e '>' + Next-Hop + MED + LocPrf + AS-Path)
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        let mut paths = Vec::new();
        // Lines that look like routes but could not be read. Reported as a
        // partial result rather than silently dropped (ADR-0006).
        let mut unreadable = 0usize;
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
            let header = line.trim_start();
            if header.is_empty()
                || header.starts_with("Total")
                || header.starts_with("BGP")
                || header.starts_with("Status")
                || header.starts_with("Network")
                || header.starts_with("Route Flag")
                || header.starts_with("Paths:")
            {
                continue;
            }

            // Checar se a linha começa com status de rota (*, *>, etc.)
            if line.starts_with('*')
                || line.starts_with('>')
                || line.starts_with(" *")
                || line.starts_with(" *>")
            {
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
                    let next_hop_text = parts[idx];
                    idx += 1;

                    let med = parts.get(idx).and_then(|v| v.parse().ok());
                    idx += 1;

                    let local_pref = parts.get(idx).and_then(|v| v.parse().ok());
                    idx += 1; // PrefVal
                    idx += 1;

                    // AS path and origin. VRP marks the origin with a trailing
                    // i (IGP), e (EGP) or ? (incomplete) on the last token. When
                    // no marker is present the router did not tell us, so the
                    // origin stays None instead of being assumed to be IGP.
                    let mut as_path = Vec::new();
                    let mut origin: Option<Origin> = None;

                    for &token in &parts[idx.min(parts.len())..] {
                        let last = token.chars().last();
                        let marker = last.and_then(Origin::from_marker);
                        let clean = match marker {
                            Some(_) => &token[..token.len() - last.map_or(0, |c| c.len_utf8())],
                            None => token,
                        };
                        if let Some(m) = marker {
                            origin = Some(m);
                        }
                        if let Ok(asn) = clean.parse::<u32>() {
                            as_path.push(asn);
                        }
                    }

                    let prefix = parse_network(&current_network);
                    let next_hop = parse_hop(next_hop_text);
                    if prefix.is_none() && next_hop.is_none() {
                        // Looked like a route line but carried neither a
                        // network nor a next hop: report it as unreadable
                        // instead of emitting an empty path.
                        unreadable += 1;
                        continue;
                    }

                    paths.push(BgpPath {
                        is_best,
                        is_valid: Some(line.trim_start().starts_with('*')),
                        prefix,
                        next_hop,
                        peer: None,
                        as_path,
                        local_pref,
                        med,
                        weight: None,
                        origin,
                        communities: Vec::new(),
                        rpki: RpkiValidation::default(),
                    });
                } else {
                    unreadable += 1;
                }
            } else {
                unreadable += 1;
            }
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
