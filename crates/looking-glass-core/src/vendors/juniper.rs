use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, Community, DriverError,
    Origin, PingResult, QueryTarget, RpkiStatus, RpkiValidation, TracerouteResult, VendorDriver,
};
use serde::Deserialize;
use std::net::IpAddr;

pub struct JuniperDriver;

#[derive(Deserialize, Debug)]
struct JunosRouteInformation {
    #[serde(rename = "route-information")]
    route_information: Option<Vec<JunosRouteTableContainer>>,
}

#[derive(Deserialize, Debug)]
struct JunosRouteTableContainer {
    #[serde(rename = "route-table")]
    route_table: Option<Vec<JunosRouteTable>>,
}

#[derive(Deserialize, Debug)]
struct JunosRouteTable {
    rt: Option<Vec<JunosRt>>,
}

#[derive(Deserialize, Debug)]
struct JunosRt {
    #[serde(rename = "rt-destination")]
    rt_destination: Option<Vec<JunosText>>,
    #[serde(rename = "rt-entry")]
    rt_entry: Option<Vec<JunosRtEntry>>,
}

#[derive(Deserialize, Debug)]
struct JunosRtEntry {
    #[serde(rename = "active-tag")]
    active_tag: Option<Vec<JunosText>>,
    #[serde(rename = "validation-state")]
    validation_state: Option<Vec<JunosText>>,
    #[serde(rename = "as-path")]
    as_path: Option<Vec<JunosText>>,
    #[serde(rename = "local-preference")]
    local_preference: Option<Vec<JunosText>>,
    metric: Option<Vec<JunosText>>,
    nh: Option<Vec<JunosNh>>,
    communities: Option<Vec<JunosCommunity>>,
}

#[derive(Deserialize, Debug)]
struct JunosCommunity {
    community: Option<Vec<JunosText>>,
}

#[derive(Deserialize, Debug)]
struct JunosNh {
    #[serde(rename = "to")]
    to: Option<Vec<JunosText>>,
}

#[derive(Deserialize, Debug)]
struct JunosText {
    data: String,
}

impl VendorDriver for JuniperDriver {
    fn vendor_name(&self) -> &'static str {
        "juniper_junos"
    }

    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("set cli screen-length 0")
    }

    fn prompt_pattern(&self) -> &'static str {
        r"[\w\.\-]+[>#%]\s*$"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping {} count {} no-resolve", target, count.clamp(1, 20))
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("traceroute {} no-resolve", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        match target {
            QueryTarget::Ip(ip) => format!("show route {} detail | display json", ip),
            QueryTarget::Prefix(net) => format!("show route {} detail | display json", net),
            QueryTarget::Asn(asn) => format!(
                "show route aspath-regex \".*{}.*\" detail | display json",
                asn
            ),
        }
    }

    fn format_bgp_summary(&self) -> String {
        "show bgp summary | display json".to_string()
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        // Ex: 5 packets transmitted, 5 packets received, 0% packet loss
        // round-trip min/avg/max/stddev = 1.123/1.456/2.012/0.123 ms
        let mut sent = 0;
        let mut recv = 0;
        let mut loss = 100.0;
        let mut min = None;
        let mut avg = None;
        let mut max = None;

        for line in raw.lines() {
            if line.contains("packets transmitted") {
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
                    let clean = p.replace('%', "");
                    if let Some(num) = clean.split_whitespace().next() {
                        loss = num.parse().unwrap_or(100.0);
                    }
                }
            } else if line.contains("round-trip min/avg/max") || line.contains("rtt min/avg/max") {
                if let Some(vals) = line.split('=').nth(1) {
                    let parts: Vec<&str> = vals.trim().split('/').collect();
                    if parts.len() >= 3 {
                        min = parts[0].trim().parse().ok();
                        avg = parts[1].trim().parse().ok();
                        max = parts[2]
                            .split_whitespace()
                            .next()
                            .and_then(|s| s.parse().ok());
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

    /// Deserialização JSON Nativa do Juniper JunOS (Extração direta de RPKI e BGP Paths)
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        let parsed: JunosRouteInformation = serde_json::from_str(raw)
            .map_err(|e| DriverError::ParseError(format!("JSON do JunOS inválido: {}", e)))?;

        let mut paths = Vec::new();

        if let Some(tables_container) = parsed.route_information {
            for container in tables_container {
                if let Some(tables) = container.route_table {
                    for table in tables {
                        if let Some(rts) = table.rt {
                            for rt in rts {
                                let network = rt
                                    .rt_destination
                                    .and_then(|d| d.into_iter().next())
                                    .map(|t| t.data)
                                    .unwrap_or_default();

                                if let Some(entries) = rt.rt_entry {
                                    for entry in entries {
                                        let is_best = entry.active_tag.is_some();

                                        // RPKI validation nativo do JunOS
                                        let rpki_status = match entry
                                            .validation_state
                                            .and_then(|v| v.into_iter().next())
                                            .map(|t| t.data.to_lowercase())
                                            .as_deref()
                                        {
                                            Some("valid") => RpkiStatus::Valid,
                                            Some("invalid") => RpkiStatus::Invalid,
                                            Some("unknown") | Some("not-found") => {
                                                RpkiStatus::NotFound
                                            }
                                            _ => RpkiStatus::NotChecked,
                                        };

                                        let next_hop = entry
                                            .nh
                                            .and_then(|n| n.into_iter().next())
                                            .and_then(|n| n.to)
                                            .and_then(|t| t.into_iter().next())
                                            .and_then(|t| parse_hop(&t.data));

                                        // Junos prints "65100 65500 I": AS
                                        // numbers followed by the origin marker.
                                        let mut as_path = Vec::new();
                                        let mut origin = None;
                                        if let Some(ap_vec) = entry.as_path {
                                            if let Some(ap_txt) = ap_vec.into_iter().next() {
                                                for token in ap_txt.data.split_whitespace() {
                                                    if let Ok(asn) = token.parse::<u32>() {
                                                        as_path.push(asn);
                                                    } else if let Some(marker) = token
                                                        .chars()
                                                        .next()
                                                        .and_then(Origin::from_marker)
                                                    {
                                                        origin = Some(marker);
                                                    }
                                                }
                                            }
                                        }

                                        let local_pref = entry
                                            .local_preference
                                            .and_then(|lp| lp.into_iter().next())
                                            .and_then(|t| t.data.parse().ok());

                                        let med = entry
                                            .metric
                                            .and_then(|m| m.into_iter().next())
                                            .and_then(|t| t.data.parse().ok());

                                        let mut communities = Vec::new();
                                        if let Some(comms) = entry.communities {
                                            for c in comms {
                                                if let Some(list) = c.community {
                                                    for item in list {
                                                        communities
                                                            .push(Community::parse(&item.data));
                                                    }
                                                }
                                            }
                                        }

                                        paths.push(BgpPath {
                                            is_best,
                                            is_valid: Some(true),
                                            prefix: parse_network(&network),
                                            next_hop,
                                            as_path,
                                            local_pref,
                                            med,
                                            origin,
                                            communities,
                                            rpki: RpkiValidation::from_router(rpki_status),
                                            ..BgpPath::default()
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
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
