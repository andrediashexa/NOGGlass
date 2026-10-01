use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpPeerSummary, BgpRouteResult, BgpSummaryResult,
    DriverError, Origin, PingResult, TracerouteResult, VendorDriver,
};
use regex::Regex;
use std::net::IpAddr;
use std::sync::LazyLock;

pub struct NokiaSrosDriver;

impl VendorDriver for NokiaSrosDriver {
    fn vendor_name(&self) -> &'static str {
        "nokia_sros"
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        crate::ping::parse(raw)
    }

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
    }

    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // SR OS does not print a table. Each route is a block of lines:
        //
        // Flag  Network                              LocalPref   MED
        //       Nexthop (Router Info)                Path-Id     IGP Cost
        //       As-Path                              Label
        // -------------------------------------------------------------
        // u*>i  198.51.100.0/24                      150         10
        //       192.0.2.254                          None        10
        //       65100 65500                          -
        //
        // The flag line starts a record; the two lines under it belong to it.
        // Reading this line by line, as if it were a table, produces one broken
        // path per line instead of one good path per block.
        let mut paths = Vec::new();
        let mut unreadable = 0usize;
        let mut current: Option<(BgpPath, usize)> = None;
        let mut in_legend = false;
        let mut in_table = false;

        for line in raw.lines() {
            let body = line.trim();
            if body.is_empty()
                || body.starts_with('=')
                || body.starts_with('-')
                // The header spans three lines on SR OS; these are the other
                // two, and they look like the value lines they describe.
                || body.starts_with("Nexthop")
                || body.starts_with("As-Path")
                || body.starts_with("Status codes")
                || body.starts_with("Origin codes")
                || body.starts_with("BGP")
                || body.starts_with("No Matching Entries")
            {
                if in_legend && (body.starts_with('=') || body.starts_with("BGP") || body.starts_with("Flag")) {
                    in_legend = false;
                }
                continue;
            }

            if body.starts_with("Legend") {
                in_legend = true;
                continue;
            }

            if in_legend {
                if body.starts_with('=') || body.starts_with("BGP") || body.starts_with("Flag") {
                    in_legend = false;
                } else {
                    continue;
                }
            }

            if body.starts_with("Flag") {
                in_table = true;
                continue;
            }

            if body.starts_with("Routes :") {
                if let Some((path, _)) = current.take() {
                    paths.push(path);
                }
                in_table = false;
                continue;
            }

            let fields: Vec<&str> = body.split_whitespace().collect();

            // A flag line: flags (alphanumeric, *, >, ?, -), then the network.
            let starts_record = fields
                .first()
                .is_some_and(|flag| !flag.is_empty() && flag.chars().all(|c| c.is_ascii_alphanumeric() || "*>?-".contains(c)))
                && fields.len() >= 2
                && parse_network(fields[1]).is_some();

            if starts_record {
                in_table = true;
                if let Some((path, _)) = current.take() {
                    paths.push(path);
                }
                let flags = fields[0];
                let mut path = BgpPath {
                    prefix: parse_network(fields[1]),
                    // u is used, > is best. SR OS marks both on the route it
                    // installed.
                    is_best: flags.contains('>') || flags.contains('u'),
                    is_valid: Some(flags.contains('*')),
                    origin: flags.chars().last().and_then(Origin::from_marker),
                    ..BgpPath::default()
                };
                path.local_pref = fields.get(2).and_then(|v| v.parse().ok());
                path.med = fields.get(3).and_then(|v| v.parse().ok());
                current = Some((path, 0));
                continue;
            }

            let Some((path, seen)) = current.as_mut() else {
                if in_table {
                    unreadable += 1;
                }
                continue;
            };

            *seen += 1;
            match *seen {
                // The next-hop line.
                1 => path.next_hop = parse_hop(fields[0]),
                // The AS path line. A route this router originated prints no
                // path at all, which is why an empty one is not an error.
                2 => {
                    path.as_path = fields
                        .iter()
                        .take_while(|token| token.bytes().all(|b| b.is_ascii_digit()))
                        .filter_map(|token| token.parse().ok())
                        .collect();
                }
                _ => {}
            }
        }

        if let Some((path, _)) = current.take() {
            paths.push(path);
        }

        Ok(BgpRouteResult::new(paths, raw).partial(unreadable))
    }

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        Ok(parse_summary(raw))
    }
}

static ROUTER_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:BGP\s+)?(?:local\s+)?router\s+ID\s*:?\s*(?:is\s+)?(?P<id>\d[0-9a-fA-F:.]*)|identifier\s+(?P<id2>[0-9a-fA-F:.]+)",
    )
    .expect("router line regex")
});

static LOCAL_AS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)local\s+AS(?:\s+number)?\s*:?\s*(?P<asn>\d+)").expect("local AS regex")
});

/// Whether a token looks like an uptime duration in Nokia SR OS (e.g. `77d23h49m`, `0421d17h`, `17h33m42s`).
fn looks_like_uptime(token: &str) -> bool {
    let clean = token.trim_end_matches('*');
    let lower = clean.to_ascii_lowercase();
    if lower == "never" {
        return true;
    }
    clean.chars().any(|c| c.is_ascii_digit())
        && (clean.contains(':')
            || clean
                .chars()
                .any(|c| matches!(c.to_ascii_lowercase(), 'd' | 'h' | 'w' | 'm' | 's' | 'y')))
}

/// Parses Nokia SR OS prefix counters in format `<rcv>/<act>/<sent>` (e.g. `250511/47465/1081295`).
fn parse_slash_counts(token: &str) -> Option<(u32, u32)> {
    let clean = token.split('(').next().unwrap_or(token).trim_end_matches('*');
    let parts: Vec<&str> = clean.split('/').collect();
    if parts.len() == 3 {
        let rcv = parts[0].trim_end_matches('*').parse::<u32>().ok()?;
        let act = parts[1].trim_end_matches('*').parse::<u32>().ok()?;
        let _sent = parts[2].trim_end_matches('*').parse::<u32>().ok()?;
        Some((rcv, act))
    } else {
        None
    }
}

/// Parses an autonomous system number in plain integer or asdot (`high.low`) format.
fn parse_asn(token: &str) -> Option<u32> {
    let clean = token.trim_end_matches('*');
    if let Ok(asn) = clean.parse::<u32>() {
        return Some(asn);
    }
    if let Some((high, low)) = clean.split_once('.') {
        let h = high.parse::<u32>().ok()?;
        let l = low.parse::<u32>().ok()?;
        if h <= 65535 && l <= 65535 {
            return Some((h << 16) + l);
        }
    }
    None
}

/// Extracts a peer IP from a line if it begins with an IPv4 or IPv6 address.
fn extract_peer_ip(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_d = trimmed
        .strip_prefix("D ")
        .or_else(|| trimmed.strip_prefix("D\t"))
        .unwrap_or(trimmed)
        .trim();
    let first_token = without_d.split_whitespace().next()?;
    let candidate = first_token.trim_end_matches([':', '*']);
    if candidate.parse::<IpAddr>().is_ok() {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

struct CurrentPeer {
    ip: String,
    as_number: Option<u32>,
    uptime: String,
    state: String,
    rcv_prefixes: u32,
    act_prefixes: u32,
    has_established_counts: bool,
}

impl CurrentPeer {
    fn new(ip: String) -> Self {
        Self {
            ip,
            as_number: None,
            uptime: String::new(),
            state: String::new(),
            rcv_prefixes: 0,
            act_prefixes: 0,
            has_established_counts: false,
        }
    }

    fn into_summary(self) -> Option<BgpPeerSummary> {
        let peer_as = self.as_number?;
        let state = if self.state.is_empty() {
            if self.has_established_counts {
                "Established".to_string()
            } else {
                "Unknown".to_string()
            }
        } else {
            self.state
        };

        let prefixes_accepted = if self.has_established_counts {
            Some(self.act_prefixes)
        } else {
            None
        };

        Some(BgpPeerSummary {
            peer_ip: self.ip,
            peer_as,
            state,
            uptime: self.uptime,
            prefixes_received: self.rcv_prefixes,
            prefixes_accepted,
        })
    }
}

/// Reads a Nokia SR OS BGP summary.
///
/// Nokia SR OS prints each peer as a multi-line block under `show router bgp summary all`,
/// with IPv4/IPv6 address on the first line, description on the second, and session metrics
/// (`ServiceId AS PktRcvd InQ Up/Down State|Rcv/Act/Sent`) on subsequent lines.
pub fn parse_summary(raw: &str) -> BgpSummaryResult {
    let mut router_id = None;
    let mut local_as = None;
    let mut peers = Vec::new();
    let mut current_peer: Option<CurrentPeer> = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if router_id.is_none() {
            if let Some(captures) = ROUTER_LINE.captures(line) {
                router_id = captures
                    .name("id")
                    .or_else(|| captures.name("id2"))
                    .map(|m| m.as_str().to_string());
            }
        }
        if local_as.is_none() {
            if let Some(captures) = LOCAL_AS.captures(line) {
                local_as = captures["asn"].parse().ok();
            }
        }

        if trimmed.starts_with("====") || trimmed.starts_with("----") {
            continue;
        }

        if let Some(ip) = extract_peer_ip(trimmed) {
            if let Some(prev) = current_peer.take() {
                if let Some(summary) = prev.into_summary() {
                    peers.push(summary);
                }
            }
            current_peer = Some(CurrentPeer::new(ip));
            continue;
        }

        if let Some(ref mut peer) = current_peer {
            let tokens: Vec<&str> = trimmed.split_whitespace().collect();

            if peer.as_number.is_none() {
                if let Some(uptime_idx) = tokens.iter().position(|t| looks_like_uptime(t)) {
                    let asn_opt = if uptime_idx >= 3 {
                        parse_asn(tokens[uptime_idx - 3])
                    } else {
                        None
                    }
                    .or_else(|| {
                        tokens[..uptime_idx]
                            .iter()
                            .rev()
                            .skip(2)
                            .find_map(|t| parse_asn(t))
                    });

                    if let Some(asn) = asn_opt {
                        peer.as_number = Some(asn);
                        peer.uptime = tokens[uptime_idx].trim_end_matches('*').to_string();

                        if let Some(&state_token) = tokens.get(uptime_idx + 1) {
                            if let Some((rcv, act)) = parse_slash_counts(state_token) {
                                peer.state = "Established".to_string();
                                peer.rcv_prefixes += rcv;
                                peer.act_prefixes += act;
                                peer.has_established_counts = true;
                            } else {
                                let lower = state_token.to_ascii_lowercase();
                                peer.state = if lower.starts_with("admin")
                                    && tokens.get(uptime_idx + 2).map(|s| s.to_ascii_lowercase())
                                        == Some("down".to_string())
                                {
                                    "Admin Down".to_string()
                                } else {
                                    capitalise(state_token)
                                };
                            }
                        }
                    }
                }
            } else {
                for token in &tokens {
                    if let Some((rcv, act)) = parse_slash_counts(token) {
                        peer.rcv_prefixes += rcv;
                        peer.act_prefixes += act;
                        break;
                    }
                }
            }
        }
    }

    if let Some(prev) = current_peer.take() {
        if let Some(summary) = prev.into_summary() {
            peers.push(summary);
        }
    }

    if peers.is_empty() {
        return crate::summary::parse(raw);
    }

    BgpSummaryResult {
        router_id,
        local_as,
        peers,
        raw_output: raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{Completeness, RpkiStatus};

    /// SR OS prints each route as a block of three lines, not as a table row.
    /// Reading it line by line produces one broken path per line.
    #[test]
    fn reads_multi_line_route_blocks() {
        let raw = "\
===============================================================================
 BGP Router ID:192.0.2.1        AS:65001       Local AS:65001
===============================================================================
 Legend -
 Status codes  : u - used, s - suppressed, h - history, d - decayed, * - valid
 Origin codes  : i - IGP, e - EGP, ? - incomplete, > - best, b - backup

===============================================================================
BGP IPv4 Routes
===============================================================================
Flag  Network                                    LocalPref   MED
      Nexthop (Router Info)                      Path-Id     IGP Cost
      As-Path                                    Label
-------------------------------------------------------------------------------
u*>i  198.51.100.0/24                            150         10
      192.0.2.254                                None        10
      65100 65500                                -
*i    198.51.100.0/24                            100         20
      192.0.2.253                                None        20
      65200 65300 65500                          -
-------------------------------------------------------------------------------
";
        let result = NokiaSrosDriver
            .parse_bgp_route(raw)
            .expect("SR OS output should parse");

        assert_eq!(result.paths.len(), 2, "two blocks, two paths");
        assert_eq!(result.completeness, Completeness::Complete);

        let used = result.best().expect("the u flag marks the used route");
        assert_eq!(used.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(used.next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(used.as_path, vec![65100, 65500]);
        assert_eq!(used.local_pref, Some(150));
        assert_eq!(used.med, Some(10));
        assert_eq!(used.origin, Some(Origin::Igp));

        let backup = &result.paths[1];
        assert!(!backup.is_best);
        assert_eq!(backup.as_path, vec![65200, 65300, 65500]);
        assert_eq!(backup.local_pref, Some(100));
    }

    #[test]
    fn a_table_with_no_routes_is_a_complete_answer() {
        let raw = "\
===============================================================================
BGP IPv4 Routes
===============================================================================
No Matching Entries Found
===============================================================================
";
        let result = NokiaSrosDriver.parse_bgp_route(raw).unwrap();
        assert!(result.paths.is_empty());
        assert_eq!(result.completeness, Completeness::Complete);
        assert_eq!(result.raw_output, raw);
    }

    #[test]
    fn rpki_is_left_unchecked_because_the_parser_read_none() {
        let raw =
            "u*>i  198.51.100.0/24   150   10\n      192.0.2.254   None   10\n      65100   -\n";
        let result = NokiaSrosDriver.parse_bgp_route(raw).unwrap();
        assert_eq!(result.paths[0].rpki.status, RpkiStatus::NotChecked);
    }

    #[test]
    fn reads_the_unix_style_ping_summary() {
        let raw = "\
PING 198.51.100.1 56 data bytes
--- 198.51.100.1 ping statistics ---
5 packets transmitted, 5 packets received, 0.00% packet loss
round-trip min/avg/max = 0.412/0.501/0.688 ms
";
        let result = NokiaSrosDriver.parse_ping(raw).unwrap();
        assert_eq!(result.packets_received, 5);
        assert_eq!(result.avg_rtt_ms, Some(0.501));
    }

    #[test]
    fn reads_real_world_sros_routes_with_wrapped_legend() {
        let raw = "\
===============================================================================
 BGP Router ID:10.10.10.1      AS:52838         Local AS:52838                 
===============================================================================
 Legend -
 Status codes  : u - used, s - suppressed, h - history, d - decayed, * - valid
                 l - leaked, x - stale, > - best, b - backup, p - purge
 Origin codes  : i - IGP, e - EGP, ? - incomplete

===============================================================================
BGP IPv4 Routes
===============================================================================
Flag  Network                                           LocalPref   MED
      Nexthop (Router)                                  Path-Id     IGP Cost
      As-Path                                                       Label
-------------------------------------------------------------------------------
u*>i  1.1.1.0/24                                        None        100
      89.221.42.164                                     None        0
      6762 13335                                                    -
*i    1.1.1.0/24                                        None        11556
      200.15.9.84                                       None        0
      2914 13335                                                    -
*i    1.1.1.0/24                                        None        108711
      199.100.1.161                                     None        0
      174 13335                                                     -
-------------------------------------------------------------------------------
Routes : 3
===============================================================================
";
        let result = NokiaSrosDriver.parse_bgp_route(raw).unwrap();
        assert_eq!(result.paths.len(), 3);
        assert_eq!(result.completeness, Completeness::Complete);

        let best = result.best().expect("u*>i route is best");
        assert_eq!(best.prefix.unwrap().to_string(), "1.1.1.0/24");
        assert_eq!(best.next_hop.unwrap().to_string(), "89.221.42.164");
        assert_eq!(best.as_path, vec![6762, 13335]);
        assert_eq!(best.local_pref, None);
        assert_eq!(best.med, Some(100));
        assert_eq!(best.origin, Some(Origin::Igp));

        assert_eq!(result.paths[1].as_path, vec![2914, 13335]);
        assert_eq!(result.paths[1].med, Some(11556));

        assert_eq!(result.paths[2].as_path, vec![174, 13335]);
        assert_eq!(result.paths[2].med, Some(108711));
    }

    #[test]
    fn reads_real_world_sros_bgp_summary_all() {
        let raw = "\
===============================================================================
BGP Summary
===============================================================================
Legend : D - Dynamic Neighbor
===============================================================================
Neighbor
Description
ServiceId          AS PktRcvd InQ  Up/Down   State|Rcv/Act/Sent (Addr Family)
                      PktSent OutQ
-------------------------------------------------------------------------------
10.10.10.21
VM-RR-ARI-CBG-01
Def. Inst       52838 106198*    0 77d23h49m 250511/47465/1081295 (IPv4)
                      443797*    0           0/0/0 (VpnIPv4)
                                             114524/76659/247802 (Lbl-IPv6)
10.10.10.22
VM-RR-JPI-CEN-01
Def. Inst       52838 305985*    0 0265d01h  250511/269/1081294 (IPv4)
                      145714*    0           0/0/0 (VpnIPv4)
                                             114524/31/247799 (Lbl-IPv6)
10.45.0.2
CLI-ASN-267398-MEGAIP-V4
Def. Inst      267398    2280    0 17h33m42s 2/2/1128773 (IPv4)
                       494330    0           
10.255.240.14
CLI-ASN-267398-MEGAIP-V4
Def. Inst      267398       0    0 0421d17h  Connect
                            0    0           
45.231.234.6
route-consumers_v4
Def. Inst        2914       0    0 0421d17h  Active
                      1213050    0           
2001:504:0:7:0:2:4115:1
IX-08-EQX-SPO-SP4-V6
Def. Inst       24115 177385*    0 63d12h46m 81695/2709/7 (IPv6)
                        98037    0           
2804:bd8:fafb::2
CLI-ASN-267398-MEGAIP-V6
Def. Inst      267398 1198876    0 0421d17h  Active
                      2203301    0           
2804:bd8:fafb::16
CLI-ASN-267398-MEGAIP-V6
Def. Inst      267398    2276    0 17h34m21s 3/3/324434 (IPv6)
                       307976    0           
-------------------------------------------------------------------------------
";
        let result = NokiaSrosDriver.parse_bgp_summary(raw).unwrap();
        assert_eq!(result.peers.len(), 8);

        // Peer 1: multi-family (IPv4 + Lbl-IPv6)
        let p1 = &result.peers[0];
        assert_eq!(p1.peer_ip, "10.10.10.21");
        assert_eq!(p1.peer_as, 52838);
        assert_eq!(p1.state, "Established");
        assert_eq!(p1.uptime, "77d23h49m");
        assert_eq!(p1.prefixes_received, 250511 + 114524);
        assert_eq!(p1.prefixes_accepted, Some(47465 + 76659));

        // Peer 2: multi-family (IPv4 + Lbl-IPv6)
        let p2 = &result.peers[1];
        assert_eq!(p2.peer_ip, "10.10.10.22");
        assert_eq!(p2.peer_as, 52838);
        assert_eq!(p2.state, "Established");
        assert_eq!(p2.uptime, "0265d01h");
        assert_eq!(p2.prefixes_received, 250511 + 114524);
        assert_eq!(p2.prefixes_accepted, Some(269 + 31));

        // Peer 3: IPv4
        let p3 = &result.peers[2];
        assert_eq!(p3.peer_ip, "10.45.0.2");
        assert_eq!(p3.peer_as, 267398);
        assert_eq!(p3.state, "Established");
        assert_eq!(p3.uptime, "17h33m42s");
        assert_eq!(p3.prefixes_received, 2);
        assert_eq!(p3.prefixes_accepted, Some(2));

        // Peer 4: Connect
        let p4 = &result.peers[3];
        assert_eq!(p4.peer_ip, "10.255.240.14");
        assert_eq!(p4.peer_as, 267398);
        assert_eq!(p4.state, "Connect");
        assert_eq!(p4.uptime, "0421d17h");
        assert_eq!(p4.prefixes_received, 0);
        assert_eq!(p4.prefixes_accepted, None);

        // Peer 5: Active
        let p5 = &result.peers[4];
        assert_eq!(p5.peer_ip, "45.231.234.6");
        assert_eq!(p5.peer_as, 2914);
        assert_eq!(p5.state, "Active");
        assert_eq!(p5.uptime, "0421d17h");
        assert_eq!(p5.prefixes_received, 0);
        assert_eq!(p5.prefixes_accepted, None);

        // Peer 6: IPv6
        let p6 = &result.peers[5];
        assert_eq!(p6.peer_ip, "2001:504:0:7:0:2:4115:1");
        assert_eq!(p6.peer_as, 24115);
        assert_eq!(p6.state, "Established");
        assert_eq!(p6.uptime, "63d12h46m");
        assert_eq!(p6.prefixes_received, 81695);
        assert_eq!(p6.prefixes_accepted, Some(2709));

        // Peer 7: IPv6 Active
        let p7 = &result.peers[6];
        assert_eq!(p7.peer_ip, "2804:bd8:fafb::2");
        assert_eq!(p7.peer_as, 267398);
        assert_eq!(p7.state, "Active");
        assert_eq!(p7.uptime, "0421d17h");
        assert_eq!(p7.prefixes_received, 0);
        assert_eq!(p7.prefixes_accepted, None);

        // Peer 8: IPv6 Established
        let p8 = &result.peers[7];
        assert_eq!(p8.peer_ip, "2804:bd8:fafb::16");
        assert_eq!(p8.peer_as, 267398);
        assert_eq!(p8.state, "Established");
        assert_eq!(p8.uptime, "17h34m21s");
        assert_eq!(p8.prefixes_received, 3);
        assert_eq!(p8.prefixes_accepted, Some(3));
    }
}
