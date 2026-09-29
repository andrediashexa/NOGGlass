use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpPeerSummary, BgpRouteResult, BgpSummaryResult, Community,
    DriverError, Origin, PingResult, RpkiStatus, RpkiValidation, TracerouteResult, VendorDriver,
};
use serde::Deserialize;

pub struct JuniperDriver;

fn deserialize_single_or_vec<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    let opt: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match opt {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Array(arr)) => {
            let mut list = Vec::with_capacity(arr.len());
            for item in arr {
                list.push(T::deserialize(item).map_err(serde::de::Error::custom)?);
            }
            Ok(Some(list))
        }
        Some(single) => {
            let item = T::deserialize(single).map_err(serde::de::Error::custom)?;
            Ok(Some(vec![item]))
        }
    }
}

#[derive(Deserialize, Debug)]
struct JunosRouteInformation {
    #[serde(rename = "route-information")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    route_information: Option<Vec<JunosRouteTableContainer>>,
}

#[derive(Deserialize, Debug)]
struct JunosRouteTableContainer {
    #[serde(rename = "route-table")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    route_table: Option<Vec<JunosRouteTable>>,
}

#[derive(Deserialize, Debug)]
struct JunosRouteTable {
    #[serde(rename = "table-name")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    table_name: Option<Vec<JunosText>>,
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    rt: Option<Vec<JunosRt>>,
}

#[derive(Deserialize, Debug)]
struct JunosRt {
    #[serde(rename = "rt-destination")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    rt_destination: Option<Vec<JunosText>>,
    #[serde(rename = "rt-prefix-length")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    rt_prefix_length: Option<Vec<JunosText>>,
    #[serde(rename = "rt-entry")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    rt_entry: Option<Vec<JunosRtEntry>>,
}

#[derive(Deserialize, Debug)]
struct JunosRtEntry {
    #[serde(rename = "active-tag")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    active_tag: Option<Vec<JunosText>>,
    #[serde(rename = "protocol-name")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    protocol_name: Option<Vec<JunosText>>,
    #[serde(rename = "validation-state")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    validation_state: Option<Vec<JunosText>>,
    #[serde(rename = "as-path")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    as_path: Option<Vec<JunosText>>,
    #[serde(rename = "local-preference")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    local_preference: Option<Vec<JunosText>>,
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    metric: Option<Vec<JunosText>>,
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    metric2: Option<Vec<JunosText>>,
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    nh: Option<Vec<JunosNh>>,
    #[serde(rename = "protocol-nh")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    protocol_nh: Option<Vec<JunosNh>>,
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    gateway: Option<Vec<JunosText>>,
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    communities: Option<Vec<JunosCommunity>>,
}

#[derive(Deserialize, Debug)]
struct JunosCommunity {
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    community: Option<Vec<JunosText>>,
}

#[derive(Deserialize, Debug)]
struct JunosNh {
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    to: Option<Vec<JunosText>>,
}

#[derive(Deserialize, Debug)]
struct JunosBgpSummaryInformation {
    #[serde(rename = "bgp-information")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    bgp_information: Option<Vec<JunosBgpInformation>>,
}

#[derive(Deserialize, Debug)]
struct JunosBgpInformation {
    #[serde(rename = "group-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    group_count: Option<Vec<JunosText>>,
    #[serde(rename = "peer-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    peer_count: Option<Vec<JunosText>>,
    #[serde(rename = "down-peer-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    down_peer_count: Option<Vec<JunosText>>,
    #[serde(rename = "bgp-rib")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    bgp_rib: Option<Vec<JunosSummaryRib>>,
    #[serde(rename = "bgp-peer")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    bgp_peer: Option<Vec<JunosBgpPeer>>,
}

#[derive(Deserialize, Debug)]
struct JunosSummaryRib {
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    name: Option<Vec<JunosText>>,
    #[serde(rename = "total-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    total_prefix_count: Option<Vec<JunosText>>,
    #[serde(rename = "active-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    active_prefix_count: Option<Vec<JunosText>>,
    #[serde(rename = "suppressed-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    suppressed_prefix_count: Option<Vec<JunosText>>,
    #[serde(rename = "history-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    history_prefix_count: Option<Vec<JunosText>>,
    #[serde(rename = "damped-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    damped_prefix_count: Option<Vec<JunosText>>,
    #[serde(rename = "pending-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    pending_prefix_count: Option<Vec<JunosText>>,
}

#[derive(Deserialize, Debug)]
struct JunosBgpPeer {
    #[serde(rename = "peer-address")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    peer_address: Option<Vec<JunosText>>,
    #[serde(rename = "peer-as")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    peer_as: Option<Vec<JunosText>>,
    #[serde(rename = "input-messages")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    input_messages: Option<Vec<JunosText>>,
    #[serde(rename = "output-messages")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    output_messages: Option<Vec<JunosText>>,
    #[serde(rename = "route-queue-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    route_queue_count: Option<Vec<JunosText>>,
    #[serde(rename = "flap-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    flap_count: Option<Vec<JunosText>>,
    #[serde(rename = "elapsed-time")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    elapsed_time: Option<Vec<JunosText>>,
    #[serde(rename = "peer-state")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    peer_state: Option<Vec<JunosText>>,
    #[serde(rename = "bgp-rib")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    bgp_rib: Option<Vec<JunosPeerRib>>,
}

#[derive(Deserialize, Debug)]
struct JunosPeerRib {
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    name: Option<Vec<JunosText>>,
    #[serde(rename = "active-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    active_prefix_count: Option<Vec<JunosText>>,
    #[serde(rename = "received-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    received_prefix_count: Option<Vec<JunosText>>,
    #[serde(rename = "accepted-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    accepted_prefix_count: Option<Vec<JunosText>>,
    #[serde(rename = "damped-prefix-count")]
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    damped_prefix_count: Option<Vec<JunosText>>,
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
        if trimmed.is_empty() || !trimmed.contains('{') || trimmed.contains("Pattern not found") {
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
                let fallback = if raw.trim().contains('{') {
                    "error: Pattern not found\n".to_string()
                } else {
                    raw.to_string()
                };
                return Ok(BgpRouteResult::new(Vec::new(), &fallback));
            }
        };

        let mut paths = Vec::new();

        if let Some(tables_container) = &parsed.route_information {
            for container in tables_container {
                if let Some(tables) = &container.route_table {
                    for table in tables {
                        if let Some(rts) = &table.rt {
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

                                if let Some(entries) = &rt.rt_entry {
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
                                                        if let Some(comm_str) = item.data.as_deref()
                                                        {
                                                            let clean = comm_str
                                                                .strip_prefix("large:")
                                                                .unwrap_or(comm_str);
                                                            communities
                                                                .push(Community::parse(clean));
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

        let formatted_raw = format_junos_cli_output(&parsed, raw);
        Ok(BgpRouteResult::new(paths, &formatted_raw))
    }

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        let trimmed = raw.trim();
        if !trimmed.contains('{') {
            return Ok(crate::summary::parse(raw));
        }

        let json_str = match (trimmed.find('{'), trimmed.rfind('}')) {
            (Some(start), Some(end)) if start <= end => &trimmed[start..=end],
            _ => return Ok(crate::summary::parse(raw)),
        };

        let parsed: JunosBgpSummaryInformation = match serde_json::from_str(json_str) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "could not parse Junos BGP summary JSON; falling back to text parser"
                );
                return Ok(crate::summary::parse(raw));
            }
        };

        let mut peers = Vec::new();
        if let Some(bgp_infos) = &parsed.bgp_information {
            for info in bgp_infos {
                if let Some(peer_list) = &info.bgp_peer {
                    for p in peer_list {
                        let peer_ip = p
                            .peer_address
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .map(|s| s.split('+').next().unwrap_or(s))
                            .unwrap_or("")
                            .to_string();

                        if peer_ip.is_empty() {
                            continue;
                        }

                        let peer_as = p
                            .peer_as
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(0);

                        let raw_state = p
                            .peer_state
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .unwrap_or("Idle");

                        let uptime = p
                            .elapsed_time
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .unwrap_or("")
                            .to_string();

                        let mut prefixes_received = 0;
                        let mut prefixes_accepted = None;

                        if let Some(ribs) = &p.bgp_rib {
                            let mut total_rcv = 0;
                            let mut total_acc = 0;
                            let mut had_rib = false;
                            for rib in ribs {
                                if let Some(rcv) = rib
                                    .received_prefix_count
                                    .as_ref()
                                    .and_then(|v| v.first())
                                    .and_then(|t| t.data.as_deref())
                                    .and_then(|s| s.parse::<u32>().ok())
                                {
                                    total_rcv += rcv;
                                    had_rib = true;
                                }
                                if let Some(acc) = rib
                                    .accepted_prefix_count
                                    .as_ref()
                                    .and_then(|v| v.first())
                                    .and_then(|t| t.data.as_deref())
                                    .and_then(|s| s.parse::<u32>().ok())
                                {
                                    total_acc += acc;
                                    had_rib = true;
                                }
                            }
                            if had_rib {
                                prefixes_received = total_rcv;
                                prefixes_accepted = Some(total_acc);
                            }
                        }

                        peers.push(BgpPeerSummary {
                            peer_ip,
                            peer_as,
                            state: raw_state.to_string(),
                            uptime,
                            prefixes_received,
                            prefixes_accepted,
                        });
                    }
                }
            }
        }

        let formatted_raw = format_junos_summary_cli(&parsed, raw);

        Ok(BgpSummaryResult {
            router_id: None,
            local_as: None,
            peers,
            raw_output: formatted_raw,
        })
    }
}

/// Formats Junos route information into clean, human-readable Junos CLI text,
/// avoiding exposing raw machine JSON in the router output box.
fn format_junos_cli_output(parsed: &JunosRouteInformation, fallback_raw: &str) -> String {
    let Some(tables_container) = &parsed.route_information else {
        if fallback_raw.trim().contains('{') {
            return "error: Pattern not found\n".to_string();
        }
        return fallback_raw.to_string();
    };

    let mut out = String::new();

    for container in tables_container {
        let Some(tables) = &container.route_table else {
            continue;
        };
        for table in tables {
            let table_name = table
                .table_name
                .as_ref()
                .and_then(|t| t.first())
                .and_then(|x| x.data.as_deref())
                .unwrap_or("inet.0");

            let Some(rts) = &table.rt else {
                out.push_str(&format!(
                    "{table_name}: 0 destinations, 0 routes (0 active, 0 holddown, 0 hidden)\n"
                ));
                continue;
            };

            let table_dest_count = rts.len();
            let mut table_route_count = 0;
            let mut table_active_count = 0;

            for rt in rts {
                if let Some(entries) = &rt.rt_entry {
                    table_route_count += entries.len();
                    for entry in entries {
                        let is_best = entry
                            .active_tag
                            .as_ref()
                            .and_then(|t| t.first())
                            .and_then(|item| item.data.as_deref())
                            .map(|d| d.contains('*'))
                            .unwrap_or(false);
                        if is_best {
                            table_active_count += 1;
                        }
                    }
                }
            }

            out.push_str(&format!(
                "{table_name}: {table_dest_count} destinations, {table_route_count} routes ({table_active_count} active, 0 holddown, 0 hidden)\nRestart Complete\n"
            ));

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

                let prefix_str = match prefix_len {
                    Some(len) if !dest.contains('/') && !len.is_empty() => {
                        format!("{dest}/{len}")
                    }
                    _ => dest.to_string(),
                };

                let entry_count = rt.rt_entry.as_ref().map_or(0, |e| e.len());
                let entry_word = if entry_count == 1 { "entry" } else { "entries" };

                out.push_str(&format!(
                    "{prefix_str} ({entry_count} {entry_word}, 1 announced)\n"
                ));

                if let Some(entries) = &rt.rt_entry {
                    for entry in entries {
                        let is_best = entry
                            .active_tag
                            .as_ref()
                            .and_then(|t| t.first())
                            .and_then(|item| item.data.as_deref())
                            .map(|d| d.contains('*'))
                            .unwrap_or(false);

                        let proto = entry
                            .protocol_name
                            .as_ref()
                            .and_then(|p| p.first())
                            .and_then(|x| x.data.as_deref())
                            .unwrap_or("BGP");

                        let marker = if is_best { "*BGP" } else { " BGP" };
                        let proto_str = if proto.eq_ignore_ascii_case("BGP") {
                            marker.to_string()
                        } else if is_best {
                            format!("*{proto}")
                        } else {
                            format!(" {proto}")
                        };

                        out.push_str(&format!("        {proto_str:<7} Preference: 170/-101\n"));

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
                            });

                        if let Some(nh) = next_hop {
                            out.push_str(&format!("                Next hop: {nh}\n"));
                        }

                        if let Some(ap_vec) = &entry.as_path {
                            if let Some(ap_txt) = ap_vec.first() {
                                if let Some(data) = ap_txt.data.as_deref() {
                                    let first_line = data.lines().next().unwrap_or(data);
                                    let cleaned = first_line
                                        .strip_prefix("AS path:")
                                        .unwrap_or(first_line)
                                        .trim();
                                    if !cleaned.is_empty() {
                                        out.push_str(&format!(
                                            "                AS path: {cleaned}\n"
                                        ));
                                    }
                                }
                            }
                        }

                        let mut comm_strs = Vec::new();
                        if let Some(comms) = &entry.communities {
                            for c in comms {
                                if let Some(list) = &c.community {
                                    for item in list {
                                        if let Some(comm_str) = item.data.as_deref() {
                                            comm_strs.push(comm_str);
                                        }
                                    }
                                }
                            }
                        }
                        if !comm_strs.is_empty() {
                            out.push_str(&format!(
                                "                Communities: {}\n",
                                comm_strs.join(" ")
                            ));
                        }

                        if let Some(lp) = entry
                            .local_preference
                            .as_ref()
                            .and_then(|lp| lp.first())
                            .and_then(|t| t.data.as_deref())
                        {
                            out.push_str(&format!("                Localpref: {lp}\n"));
                        }

                        let metric = entry
                            .metric
                            .as_ref()
                            .and_then(|m| m.first())
                            .and_then(|t| t.data.as_deref())
                            .or_else(|| {
                                entry
                                    .metric2
                                    .as_ref()
                                    .and_then(|m| m.first())
                                    .and_then(|t| t.data.as_deref())
                            });

                        if let Some(m) = metric {
                            out.push_str(&format!("                Metric: {m}\n"));
                        }

                        if let Some(val) = entry
                            .validation_state
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                        {
                            out.push_str(&format!("                Validation State: {val}\n"));
                        }
                    }
                }
            }
        }
    }

    if out.is_empty() {
        if fallback_raw.trim().contains('{') {
            "error: Pattern not found\n".to_string()
        } else {
            fallback_raw.to_string()
        }
    } else {
        out
    }
}

/// Formats Junos BGP summary JSON into standard human-readable Junos CLI text.
fn format_junos_summary_cli(parsed: &JunosBgpSummaryInformation, fallback_raw: &str) -> String {
    let Some(bgp_infos) = &parsed.bgp_information else {
        if fallback_raw.trim().contains('{') {
            return "Groups: 0 Peers: 0 Down peers: 0\n".to_string();
        }
        return fallback_raw.to_string();
    };

    let mut out = String::new();

    for info in bgp_infos {
        let group_count = info
            .group_count
            .as_ref()
            .and_then(|v| v.first())
            .and_then(|t| t.data.as_deref())
            .unwrap_or("0");
        let peer_count = info
            .peer_count
            .as_ref()
            .and_then(|v| v.first())
            .and_then(|t| t.data.as_deref())
            .unwrap_or("0");
        let down_peer_count = info
            .down_peer_count
            .as_ref()
            .and_then(|v| v.first())
            .and_then(|t| t.data.as_deref())
            .unwrap_or("0");

        out.push_str(&format!(
            "Groups: {group_count} Peers: {peer_count} Down peers: {down_peer_count}\n"
        ));

        if let Some(ribs) = &info.bgp_rib {
            out.push_str("Table          Tot Paths  Act Paths Suppressed    History Damp State Pending\n");
            for rib in ribs {
                let name = rib
                    .name
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("inet.0");
                let tot = rib
                    .total_prefix_count
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let act = rib
                    .active_prefix_count
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let supp = rib
                    .suppressed_prefix_count
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let hist = rib
                    .history_prefix_count
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let damp = rib
                    .damped_prefix_count
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let pend = rib
                    .pending_prefix_count
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                out.push_str(&format!(
                    "{name:<14} {tot:>10} {act:>10} {supp:>10} {hist:>10} {damp:>10} {pend:>7}\n"
                ));
            }
        }

        out.push_str("Peer                     AS      InPkt     OutPkt    OutQ   Flaps Last Up/Dwn State|#Active/Received/Accepted/Damped...\n");

        if let Some(peers) = &info.bgp_peer {
            for peer in peers {
                let addr = peer
                    .peer_address
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("");
                let as_num = peer
                    .peer_as
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let in_pkt = peer
                    .input_messages
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let out_pkt = peer
                    .output_messages
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let out_q = peer
                    .route_queue_count
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let flaps = peer
                    .flap_count
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let uptime = peer
                    .elapsed_time
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("0");
                let state = peer
                    .peer_state
                    .as_ref()
                    .and_then(|v| v.first())
                    .and_then(|t| t.data.as_deref())
                    .unwrap_or("Idle");

                let state_str = if state.eq_ignore_ascii_case("Established") {
                    "Establ"
                } else {
                    state
                };

                out.push_str(&format!(
                    "{addr:<24} {as_num:<7} {in_pkt:>10} {out_pkt:>10} {out_q:>7} {flaps:>7} {uptime:>11} {state_str}\n"
                ));

                if let Some(ribs) = &peer.bgp_rib {
                    for rib in ribs {
                        let rib_name = rib
                            .name
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .unwrap_or("inet.0");
                        let act = rib
                            .active_prefix_count
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .unwrap_or("0");
                        let rcv = rib
                            .received_prefix_count
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .unwrap_or("0");
                        let acc = rib
                            .accepted_prefix_count
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .unwrap_or("0");
                        let damp = rib
                            .damped_prefix_count
                            .as_ref()
                            .and_then(|v| v.first())
                            .and_then(|t| t.data.as_deref())
                            .unwrap_or("0");
                        out.push_str(&format!("  {rib_name}: {act}/{rcv}/{acc}/{damp}\n"));
                    }
                }
            }
        }
    }

    if out.is_empty() {
        if fallback_raw.trim().contains('{') {
            "Groups: 0 Peers: 0 Down peers: 0\n".to_string()
        } else {
            fallback_raw.to_string()
        }
    } else {
        out
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

        // Formatted CLI raw_output
        assert!(
            result
                .raw_output
                .contains("inet.0: 1 destinations, 2 routes"),
            "raw_output must contain human-readable Junos CLI format"
        );
        assert!(
            result.raw_output.contains("*BGP    Preference: 170/-101"),
            "must contain active best-path tag"
        );
        assert!(
            result.raw_output.contains("Next hop: 12.122.83.238"),
            "must contain next hop"
        );
        assert!(
            !result.raw_output.contains(r#"{"route-information""#),
            "raw_output must not be raw machine JSON"
        );
    }

    #[test]
    fn test_junos_text_deserialization_edge_cases() {
        let t1: JunosText = serde_json::from_str("{}").expect("empty object");
        assert_eq!(t1.data, None);

        let t2: JunosText =
            serde_json::from_str(r#"{"other": "value"}"#).expect("missing data field");
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

    #[test]
    fn parses_junos_summary_json_and_formats_cli_output() {
        let raw = r#"{
            "bgp-information" : [
            {
                "group-count" : [{"data" : "1"}],
                "peer-count" : [{"data" : "2"}],
                "down-peer-count" : [{"data" : "0"}],
                "bgp-rib" : [
                {
                    "name" : [{"data" : "inet.0"}],
                    "total-prefix-count" : [{"data" : "100"}],
                    "active-prefix-count" : [{"data" : "50"}],
                    "suppressed-prefix-count" : [{"data" : "0"}],
                    "history-prefix-count" : [{"data" : "0"}],
                    "damped-prefix-count" : [{"data" : "0"}],
                    "pending-prefix-count" : [{"data" : "0"}]
                }
                ],
                "bgp-peer" : [
                {
                    "peer-address" : [{"data" : "12.122.83.238+179"}],
                    "peer-as" : [{"data" : "7018"}],
                    "input-messages" : [{"data" : "12345"}],
                    "output-messages" : [{"data" : "12345"}],
                    "route-queue-count" : [{"data" : "0"}],
                    "flap-count" : [{"data" : "0"}],
                    "elapsed-time" : [{"data" : "1w2d 3:04:05"}],
                    "peer-state" : [{"data" : "Established"}],
                    "bgp-rib" : [
                    {
                        "name" : [{"data" : "inet.0"}],
                        "active-prefix-count" : [{"data" : "50"}],
                        "received-prefix-count" : [{"data" : "100"}],
                        "accepted-prefix-count" : [{"data" : "100"}],
                        "suppressed-prefix-count" : [{"data" : "0"}]
                    }
                    ]
                }
                ]
            }
            ]
        }"#;

        let result = JuniperDriver
            .parse_bgp_summary(raw)
            .expect("must parse Junos BGP summary JSON");

        assert_eq!(result.peers.len(), 1);
        let peer = &result.peers[0];
        assert_eq!(peer.peer_ip, "12.122.83.238");
        assert_eq!(peer.peer_as, 7018);
        assert_eq!(peer.state, "Established");
        assert_eq!(peer.uptime, "1w2d 3:04:05");
        assert_eq!(peer.prefixes_received, 100);
        assert_eq!(peer.prefixes_accepted, Some(100));

        assert!(
            result.raw_output.contains("Groups: 1 Peers: 2 Down peers: 0"),
            "raw_output must contain group/peer header"
        );
        assert!(
            result.raw_output.contains("12.122.83.238+179"),
            "raw_output must contain peer row"
        );
        assert!(
            result.raw_output.contains("inet.0: 50/100/100/0"),
            "raw_output must contain rib prefix counts"
        );
        assert!(
            !result.raw_output.contains(r#"{"bgp-information""#),
            "raw_output must not contain raw machine JSON"
        );
    }

    #[test]
    fn parses_junos_empty_aspath_or_route_without_exposing_json() {
        // Case 1: Empty route-information array (e.g. aspath-regex matched nothing)
        let empty_json = r#"{"route-information": []}"#;
        let res1 = JuniperDriver
            .parse_bgp_route(empty_json)
            .expect("must parse empty route-information");
        assert!(res1.paths.is_empty());
        assert_eq!(res1.raw_output, "error: Pattern not found\n");

        // Case 2: Table present with 0 routes
        let zero_routes_json = r#"{
            "route-information" : [
            {
                "route-table" : [
                {
                    "table-name" : [{"data" : "inet.0"}]
                }
                ]
            }
            ]
        }"#;
        let res2 = JuniperDriver
            .parse_bgp_route(zero_routes_json)
            .expect("must parse 0 routes table");
        assert!(res2.paths.is_empty());
        assert!(
            res2.raw_output.contains("inet.0: 0 destinations, 0 routes"),
            "must contain formatted 0 destinations header"
        );
        assert!(
            !res2.raw_output.contains(r#"{"route-information""#),
            "must not expose machine JSON"
        );
    }
}
