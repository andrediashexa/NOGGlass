use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, Community, DriverError,
    Origin, PingResult, RpkiStatus, RpkiValidation, TracerouteResult, VendorDriver,
};
use serde::Deserialize;

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
    #[serde(rename = "rt-prefix-length")]
    rt_prefix_length: Option<Vec<JunosText>>,
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
    metric2: Option<Vec<JunosText>>,
    nh: Option<Vec<JunosNh>>,
    #[serde(rename = "protocol-nh")]
    protocol_nh: Option<Vec<JunosNh>>,
    gateway: Option<Vec<JunosText>>,
    communities: Option<Vec<JunosCommunity>>,
}

#[derive(Deserialize, Debug)]
struct JunosCommunity {
    community: Option<Vec<JunosText>>,
}

#[derive(Deserialize, Debug)]
struct JunosNh {
    to: Option<Vec<JunosText>>,
}

#[derive(Deserialize, Debug, Default, Clone)]
struct JunosText {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    data: Option<String>,
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(match v {
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    })
}

impl VendorDriver for JuniperDriver {
    fn vendor_name(&self) -> &'static str {
        "juniper_junos"
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

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
    }

    /// Reads the structured output Junos produces with `| display json`,
    /// including the RPKI validation state it reports natively.
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        let trimmed = raw.trim();
        if trimmed.is_empty()
            || !trimmed.contains('{')
            || trimmed.contains("Pattern not found")
        {
            return Ok(BgpRouteResult::new(Vec::new(), raw));
        }

        let json_str = match (trimmed.find('{'), trimmed.rfind('}')) {
            (Some(start), Some(end)) if start <= end => &trimmed[start..=end],
            _ => return Ok(BgpRouteResult::new(Vec::new(), raw)),
        };

        let parsed: JunosRouteInformation = match serde_json::from_str(json_str) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "could not parse Junos JSON; returning empty routes with raw output preserved"
                );
                return Ok(BgpRouteResult::new(Vec::new(), raw));
            }
        };

        let mut paths = Vec::new();

        if let Some(tables_container) = parsed.route_information {
            for container in tables_container {
                if let Some(tables) = container.route_table {
                    for table in tables {
                        if let Some(rts) = table.rt {
                            for rt in rts {
                                let dest = rt
                                    .rt_destination
                                    .as_ref()
                                    .and_then(|d| d.first())
                                    .and_then(|t| t.data.as_deref())
                                    .unwrap_or_default();

                                let prefix_len = rt
                                    .rt_prefix_length
                                    .as_ref()
                                    .and_then(|d| d.first())
                                    .and_then(|t| t.data.as_deref());

                                let network_str = match prefix_len {
                                    Some(len) if !dest.contains('/') && !len.is_empty() => {
                                        format!("{dest}/{len}")
                                    }
                                    _ => dest.to_string(),
                                };
                                let prefix = parse_network(&network_str);

                                if let Some(entries) = rt.rt_entry {
                                    for entry in entries {
                                        // In Junos JSON, active-tag contains {"data": "*"} for the best path,
                                        // and empty object {} or absent for inactive paths.
                                        let is_best = entry
                                            .active_tag
                                            .as_ref()
                                            .and_then(|t| t.first())
                                            .and_then(|item| item.data.as_deref())
                                            .map(|d| d.contains('*'))
                                            .unwrap_or(false);

                                        // RPKI validation native to Junos
                                        let rpki_status = match entry
                                            .validation_state
                                            .as_ref()
                                            .and_then(|v| v.first())
                                            .and_then(|t| t.data.as_deref())
                                            .map(str::to_lowercase)
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
                                            .as_ref()
                                            .and_then(|n| n.first())
                                            .and_then(|n| n.to.as_ref())
                                            .and_then(|t| t.first())
                                            .and_then(|t| t.data.as_deref())
                                            .or_else(|| {
                                                entry
                                                    .protocol_nh
                                                    .as_ref()
                                                    .and_then(|n| n.first())
                                                    .and_then(|n| n.to.as_ref())
                                                    .and_then(|t| t.first())
                                                    .and_then(|t| t.data.as_deref())
                                            })
                                            .or_else(|| {
                                                entry
                                                    .gateway
                                                    .as_ref()
                                                    .and_then(|g| g.first())
                                                    .and_then(|t| t.data.as_deref())
                                            })
                                            .and_then(parse_hop);

                                        // Junos prints "65100 65500 I" or "AS path: 7018 13335 I (Atomic)\nAggregator: 13335 10.34.36.200".
                                        // We parse only the primary AS path line and ignore aggregator / atomic tags.
                                        let mut as_path = Vec::new();
                                        let mut origin = None;
                                        if let Some(ap_vec) = &entry.as_path {
                                            if let Some(ap_txt) = ap_vec.first() {
                                                if let Some(data) = ap_txt.data.as_deref() {
                                                    let first_line =
                                                        data.lines().next().unwrap_or(data);
                                                    let cleaned = first_line
                                                        .strip_prefix("AS path:")
                                                        .unwrap_or(first_line);
                                                    for token in cleaned.split_whitespace() {
                                                        if let Ok(asn) = token.parse::<u32>() {
                                                            as_path.push(asn);
                                                        } else if origin.is_none() {
                                                            if let Some(marker) = token
                                                                .chars()
                                                                .next()
                                                                .and_then(Origin::from_marker)
                                                            {
                                                                origin = Some(marker);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        let local_pref = entry
                                            .local_preference
                                            .as_ref()
                                            .and_then(|lp| lp.first())
                                            .and_then(|t| t.data.as_deref())
                                            .and_then(|s| s.parse().ok());

                                        let med = entry
                                            .metric
                                            .as_ref()
                                            .and_then(|m| m.first())
                                            .and_then(|t| t.data.as_deref())
                                            .and_then(|s| s.parse().ok())
                                            .or_else(|| {
                                                entry
                                                    .metric2
                                                    .as_ref()
                                                    .and_then(|m| m.first())
                                                    .and_then(|t| t.data.as_deref())
                                                    .and_then(|s| s.parse().ok())
                                            });

                                        let mut communities = Vec::new();
                                        if let Some(comms) = &entry.communities {
                                            for c in comms {
                                                if let Some(list) = &c.community {
                                                    for item in list {
                                                        if let Some(comm_str) =
                                                            item.data.as_deref()
                                                        {
                                                            let clean = comm_str
                                                                .strip_prefix("large:")
                                                                .unwrap_or(comm_str);
                                                            communities.push(Community::parse(
                                                                clean,
                                                            ));
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        paths.push(BgpPath {
                                            is_best,
                                            is_valid: Some(true),
                                            prefix,
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

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        // Shared reader: the tables differ in headers, not in what a row means.
        Ok(crate::summary::parse(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{CommunityKind, Origin, RpkiSource, RpkiStatus};

    #[test]
    fn parses_junos_json_with_active_and_inactive_paths() {
        let raw = r#"{
            "route-information" : [
            {
                "route-table" : [
                {
                    "table-name" : [{"data" : "inet.0"}],
                    "rt" : [
                    {
                        "rt-destination" : [{"data" : "1.1.1.0"}],
                        "rt-prefix-length" : [{"data" : "24"}],
                        "rt-entry" : [
                        {
                            "active-tag" : [{"data" : "*"}],
                            "protocol-name" : [{"data" : "BGP"}],
                            "gateway" : [{"data" : "12.122.83.238"}],
                            "validation-state" : [{"data" : "valid"}],
                            "as-path" : [{"data" : "AS path: 7018 13335 I (Atomic)\nAggregator: 13335 10.34.36.200"}],
                            "communities" : [{"community" : [{"data" : "7018:2500"}, {"data" : "large:13335:10000017:0"}]}],
                            "local-preference" : [{"data" : "100"}],
                            "metric2" : [{"data" : "0"}]
                        },
                        {
                            "active-tag" : [{}],
                            "protocol-name" : [{"data" : "BGP"}],
                            "protocol-nh" : [{"to" : [{"data" : "12.122.120.7"}]}],
                            "validation-state" : [{"data" : "valid"}],
                            "as-path" : [{"data" : "AS path: 7018 13335 E"}],
                            "local-preference" : [{"data" : "100"}]
                        }
                        ]
                    }
                    ]
                }
                ]
            }
            ]
        }"#;

        let result = JuniperDriver
            .parse_bgp_route(raw)
            .expect("must parse Junos JSON without error");

        assert_eq!(result.paths.len(), 2);

        // Path 0: active (best) path
        let p0 = &result.paths[0];
        assert!(p0.is_best, "path 0 must be marked best");
        assert_eq!(p0.prefix.unwrap().to_string(), "1.1.1.0/24");
        assert_eq!(p0.next_hop.unwrap().to_string(), "12.122.83.238");
        assert_eq!(p0.as_path, vec![7018, 13335]);
        assert_eq!(p0.origin, Some(Origin::Igp));
        assert_eq!(p0.local_pref, Some(100));
        assert_eq!(p0.med, Some(0));
        assert_eq!(p0.rpki.status, RpkiStatus::Valid);
        assert_eq!(p0.rpki.source, RpkiSource::Router);
        assert_eq!(p0.communities.len(), 2);
        assert_eq!(p0.communities[0].raw, "7018:2500");
        assert_eq!(p0.communities[0].kind, CommunityKind::Standard);
        assert_eq!(p0.communities[1].raw, "13335:10000017:0");
        assert_eq!(p0.communities[1].kind, CommunityKind::Large);

        // Path 1: inactive path
        let p1 = &result.paths[1];
        assert!(!p1.is_best, "path 1 must NOT be marked best");
        assert_eq!(p1.next_hop.unwrap().to_string(), "12.122.120.7");
        assert_eq!(p1.as_path, vec![7018, 13335]);
        assert_eq!(p1.origin, Some(Origin::Egp));
    }

    #[test]
    fn test_junos_text_deserialization_edge_cases() {
        let t1: JunosText = serde_json::from_str("{}").expect("empty object");
        assert_eq!(t1.data, None);

        let t2: JunosText = serde_json::from_str(r#"{"other": "value"}"#).expect("missing data field");
        assert_eq!(t2.data, None);

        let t3: JunosText = serde_json::from_str(r#"{"data": null}"#).expect("null data");
        assert_eq!(t3.data, None);

        let t4: JunosText = serde_json::from_str(r#"{"data": 12345}"#).expect("number data");
        assert_eq!(t4.data, Some("12345".to_string()));
    }

    #[test]
    fn parses_empty_and_pattern_not_found_cleanly() {
        let empty_result = JuniperDriver
            .parse_bgp_route("")
            .expect("must handle empty response without error");
        assert!(empty_result.paths.is_empty());

        let whitespace_result = JuniperDriver
            .parse_bgp_route("   \n\t  ")
            .expect("must handle whitespace response without error");
        assert!(whitespace_result.paths.is_empty());

        let not_found_result = JuniperDriver
            .parse_bgp_route("error: Pattern not found\n")
            .expect("must handle pattern not found without error");
        assert!(not_found_result.paths.is_empty());
    }
}

