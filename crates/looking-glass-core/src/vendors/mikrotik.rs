use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpPeerSummary, BgpRouteResult, BgpSummaryResult, Community,
    DriverError, Origin, PingResult, TracerouteResult, VendorDriver,
};

/// Reads MikroTik RouterOS.
///
/// The parsing here was checked against **RouterOS 7.16.2**, and that is the
/// only version it has ever read. RouterOS 6 keeps BGP somewhere else entirely
/// — `/routing/bgp/peer` rather than `/routing/bgp/connection` — and may well
/// print it differently. There is no branch for it, because there is no
/// capture from it: if version 6 turns out to differ, that is a bug report
/// with a capture attached, and the branch will exist for a reason rather than
/// in case (ADR-0015).
///
/// This type carried an `is_v7` flag that nothing read, which promised exactly
/// that branch.
pub struct MikrotikDriver;

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
        let raw = &normalise(raw);
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
        // RouterOS redraws the whole table as it discovers hops, so the same
        // hop appears several times: once per redraw. The early ones are a
        // guess in progress — hop 1 shows `0ms` before the probe has timed out
        // — and reading them reports a round trip for a hop that never
        // answered. Only the last table is the answer.
        //
        // The shared reader handles the rest: vendors differ in decoration,
        // not in substance, and a silent hop has to survive in every one.
        let normalised = normalise(raw);
        let final_table = match normalised.rfind("Columns:") {
            Some(start) => &normalised[start..],
            None => normalised.as_str(),
        };
        Ok(crate::traceroute::parse("", final_table))
    }

    /// Reads RouterOS's key=value blocks (`/routing/route/print detail`).
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // RouterOS prints `/routing/route/print detail` as numbered blocks:
        //
        // 0 ADb dst=198.51.100.0/24 gateway=192.0.2.254 as-path=65100,65500 ...
        //
        // The letters after the index are flags, and only those count as flags:
        // scanning the whole block for a letter matched values too, which made
        // almost every route look like the best one.
        let raw_normalised = normalise(raw);
        let mut paths = Vec::new();
        let mut unreadable = 0;

        for block in raw_normalised.split("\n\n") {
            // RouterOS prints the flag legend first and does not put a blank
            // line after it, so the legend arrives glued to the first entry.
            // Skipping any block that starts with `Flags:` therefore threw away
            // a real route along with it. Legend lines carry no `key=value`;
            // an entry's first line always does, starting with `afi=`.
            let block: String = block
                .lines()
                .skip_while(|line| !line.contains('='))
                .collect::<Vec<_>>()
                .join("\n");
            let block = block.trim();
            if block.is_empty() {
                continue;
            }

            let mut path = BgpPath::default();

            // Flags come first on the entry's first line. Older RouterOS put a
            // numbered index before them; 7.16 prints none at all, and reading
            // the second token as the flags on that output found no flags ever,
            // so no route was marked active.
            let flags = read_flags(block);
            // A is active — the route the router is using. b only says the
            // route came from BGP, and every route in this output did, so
            // treating it as "best" marked all of them.
            path.is_best = flags.contains('A');
            // X is disabled; anything else is a route the router accepted.
            path.is_valid = Some(!flags.contains('X'));

            for token in block.split_whitespace() {
                // RouterOS 7 prints the BGP attributes as sub-properties:
                // `bgp.peer-cache-id=… .as-path=… .med=…`. The leading dot is
                // part of the name on every line after the first, so a parser
                // matching `as-path=` alone reads none of them.
                let token = token.strip_prefix('.').unwrap_or(token);
                let token = token.strip_prefix("bgp.").unwrap_or(token);

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
                    let (sequence, has_set) = read_as_path(val);
                    path.as_path = sequence;
                    if has_set {
                        // The model cannot express an AS_SET yet, and flattening
                        // one into the sequence would claim an order the route
                        // never had. Stop at the set and say the reading is
                        // partial; the raw output carries the whole truth.
                        unreadable += 1;
                    }
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
                } else if let Some(val) = token
                    .strip_prefix("bgp-communities=")
                    .or_else(|| token.strip_prefix("communities="))
                {
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

        Ok(BgpRouteResult::new(paths, raw).partial(unreadable))
    }

    /// Reads `/routing/bgp/session/print detail`.
    ///
    /// Not a table, so the shared reader found no sessions at all on a router
    /// with seven. RouterOS prints one numbered entry per session, with the
    /// flags after the number and the attributes as dotted sub-properties:
    ///
    ///   Flags: E - established
    ///    1 E name="peer-a-1"
    ///        remote.address=192.0.2.2 .as=64496 .id=192.0.2.2
    ///        local.address=192.0.2.1 .as=64499 .id=192.0.2.1
    ///        hold-time=3m keepalive-time=1m uptime=4m48s290ms
    ///        last-started=2026-09-20 23:13:07 prefix-count=4
    ///
    /// The dot matters: `.as=` belongs to whichever of `remote` or `local` was
    /// last named, and reading it without tracking that gives every session
    /// this router's own AS.
    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        let raw_normalised = normalise(raw);
        let mut peers: Vec<BgpPeerSummary> = Vec::new();
        let mut local_as = None;
        let mut router_id = None;
        let mut current: Option<BgpPeerSummary> = None;
        // Which group the dotted properties belong to.
        let mut group = "";

        for line in raw_normalised.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("Flags:") {
                continue;
            }

            // A new entry starts at a number, then its flags, then `name=`.
            let mut tokens = trimmed.split_whitespace().peekable();
            if tokens
                .peek()
                .is_some_and(|first| first.bytes().all(|b| b.is_ascii_digit()))
            {
                if let Some(peer) = current.take() {
                    peers.push(peer);
                }
                let established = trimmed.contains(" E ");
                current = Some(BgpPeerSummary {
                    peer_ip: String::new(),
                    peer_as: 0,
                    // `E` is the only flag RouterOS sets for a session that is
                    // up; anything else is a session that is not.
                    state: if established {
                        "Established".to_string()
                    } else {
                        "Idle".to_string()
                    },
                    uptime: String::new(),
                    prefixes_received: 0,
                    prefixes_accepted: None,
                });
                group = "";
            }

            let Some(peer) = current.as_mut() else {
                continue;
            };

            for token in trimmed.split_whitespace() {
                if let Some((key, value)) = token.split_once('=') {
                    let value = value.trim_matches('"');
                    match key {
                        "remote.address" => {
                            group = "remote";
                            peer.peer_ip = value.to_string();
                        }
                        "local.address" => group = "local",
                        ".as" => match group {
                            "remote" => peer.peer_as = value.parse().unwrap_or(0),
                            "local" => local_as = value.parse().ok(),
                            _ => {}
                        },
                        ".id" if group == "local" => router_id = Some(value.to_string()),
                        "uptime" => peer.uptime = value.to_string(),
                        "prefix-count" => peer.prefixes_received = value.parse().unwrap_or(0),
                        // A group named on its own, with its properties on the
                        // lines that follow.
                        "output.procid" => group = "output",
                        "input.procid" => group = "input",
                        _ => {}
                    }
                }
            }
        }

        if let Some(peer) = current.take() {
            peers.push(peer);
        }

        // An entry with no remote address is not a session.
        peers.retain(|peer| !peer.peer_ip.is_empty());

        Ok(BgpSummaryResult {
            router_id,
            local_as,
            peers,
            raw_output: raw.to_string(),
        })
    }
}

/// Normalises what a RouterOS SSH session actually sends.
///
/// Every line ends `\r\n`. That single carriage return is enough to empty a
/// result: splitting entries on a blank line looks for `"\n\n"`, which never
/// matches `"\r\n\r\n"`, so the whole output was read as one block, the block
/// began with the `Flags:` legend, and the parser skipped it. A router with the
/// route then answered "no route".
fn normalise(raw: &str) -> String {
    raw.replace("\r\n", "\n").replace('\r', "\n")
}

/// The flag letters at the start of an entry, if there are any.
///
/// RouterOS 7.16 prints ` Ab   afi=ip4 …`: flags first, no index. Older builds
/// print `0 ADb dst=…`. Both are read here, and anything else yields no flags
/// rather than a guess — a letter picked out of a value once marked almost
/// every route as best.
fn read_flags(block: &str) -> &str {
    let first_line = block.lines().next().unwrap_or("");
    let mut tokens = first_line.split_whitespace();
    let Some(first) = tokens.next() else {
        return "";
    };

    let is_flags =
        |token: &str| !token.is_empty() && token.chars().all(|c| c.is_ascii_alphabetic());

    // Old form: an index, then the flags.
    if first.bytes().all(|b| b.is_ascii_digit()) {
        return match tokens.next() {
            Some(flags) if is_flags(flags) => flags,
            _ => "",
        };
    }

    // Current form: the flags themselves, before the first `key=value`.
    if is_flags(first) {
        return first;
    }

    ""
}

/// Reads an AS path, stopping at an AS_SET.
///
/// RouterOS prints a set as `64498{64496,64497` — with no closing brace, which
/// is its own output and not a truncation here. The numbers inside are a set,
/// not a sequence, so they are not appended: an AS path is read left to right
/// and inventing an order for them would describe a route that does not exist.
/// The caller marks the result partial instead.
fn read_as_path(value: &str) -> (Vec<u32>, bool) {
    let value = value.trim_matches('"');
    let mut sequence = Vec::new();

    for element in value.split([',', ' ']) {
        let element = element.trim();
        if element.is_empty() {
            continue;
        }
        if element.contains('{') || element.contains('}') {
            // The sequence ends here. Whatever precedes the brace is still an
            // AS in the sequence: `64498{64496` means 64498 then a set.
            if let Some(before) = element.split('{').next() {
                if let Ok(asn) = before.trim().parse::<u32>() {
                    sequence.push(asn);
                }
            }
            return (sequence, true);
        }
        if let Ok(asn) = element.parse::<u32>() {
            sequence.push(asn);
        }
    }

    (sequence, false)
}

fn parse_mikrotik_time(s: &str) -> Option<f64> {
    // RouterOS writes a time as `1ms234us`: two units, one value.
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
        let result = MikrotikDriver
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
        let result = MikrotikDriver.parse_bgp_route(raw).unwrap();
        assert!(!result.paths[0].is_best, "the A in a comment is not a flag");
    }

    #[test]
    fn a_gateway_that_is_an_interface_is_not_a_next_hop() {
        let raw = " 0 ADb dst-address=198.51.100.0/24 gateway=ether1 as-path=\"65100\"\n";
        let result = MikrotikDriver.parse_bgp_route(raw).unwrap();
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
        let result = MikrotikDriver.parse_ping(raw).unwrap();
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
        let result = MikrotikDriver.parse_ping(raw).unwrap();
        assert_eq!(result.packets_received, 0);
        assert_eq!(result.packet_loss_percent, 100.0);
        assert_eq!(result.avg_rtt_ms, None, "there were no round trips to time");
    }

    /// RPKI is not reported by RouterOS, so it stays unchecked rather than
    /// being guessed — this is the vendor ADR-0010 exists for.
    #[test]
    fn rpki_state_is_left_unchecked() {
        let raw = " 0 ADb dst-address=198.51.100.0/24 gateway=192.0.2.254 as-path=\"65100\"\n";
        let result = MikrotikDriver.parse_bgp_route(raw).unwrap();
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
        assert_eq!(MikrotikDriver.vendor_name(), "mikrotik_routeros");
        let _ = QueryType::BgpRoute;
    }

    /// Everything below this line was captured from a RouterOS 7.16.2 in
    /// `lab/`, peering with three FRR speakers. It is what the router printed,
    /// not what its documentation describes — the difference is the reason
    /// these tests exist.
    ///
    /// RouterOS sends CRLF over SSH. The fixtures are written with plain
    /// newlines and converted here, so the conversion is visible rather than
    /// hidden in an editor.
    fn as_routeros_sends_it(text: &str) -> String {
        text.replace('\n', "\r\n")
    }

    const REAL_THREE_PATHS: &str = r#"Flags: X - disabled, F - filtered, U - unreachable, A - active; 
c - connect, s - static, r - rip, b - bgp, o - ospf, i - isis, d - dhcp, v - vpn, m - modem, a - ldp-address, l - ldp-mapping, g - slaac, y - bgp-mpls-vpn; 
H - hw-offloaded; + - ecmp, B - blackhole 
  b   afi=ip4 contribution=candidate dst-address=203.0.113.0/24 
       routing-table=main gateway=192.0.2.6 immediate-gw=192.0.2.6%ether3 
       distance=20 scope=40 target-scope=10 belongs-to="bgp-IP-192.0.2.6" 
       bgp.peer-cache-id=*2800003 .as-path="64497,64498" .med=100 .origin=igp 
       debug.fwp-ptr=0x202C24E0 

  b   afi=ip4 contribution=candidate dst-address=203.0.113.0/24 
       routing-table=main gateway=192.0.2.2 immediate-gw=192.0.2.2%ether2 
       distance=20 scope=40 target-scope=10 belongs-to="bgp-IP-192.0.2.2" 
       bgp.peer-cache-id=*2800002 
       .as-path="64496,64496,64496,65536,65537,65538,65539,65540,65541,64498" 
       .origin=igp 
       debug.fwp-ptr=0x202C2540 

 Ab   afi=ip4 contribution=active dst-address=203.0.113.0/24 
       routing-table=main gateway=192.0.2.10 immediate-gw=192.0.2.10%ether4 
       distance=20 scope=40 target-scope=10 belongs-to="bgp-IP-192.0.2.10" 
       bgp.peer-cache-id=*2800004 .as-path="64498" .med=0 .origin=igp 
       debug.fwp-ptr=0x202C2660 
"#;

    /// The carriage returns alone used to empty the result: entries are split
    /// on a blank line, `"\n\n"` never matches `"\r\n\r\n"`, so the whole
    /// output was one block, that block opened with the legend, and the parser
    /// skipped it. A router holding the route answered "no route".
    #[test]
    fn carriage_returns_do_not_empty_the_result() {
        let driver = MikrotikDriver;
        let result = driver
            .parse_bgp_route(&as_routeros_sends_it(REAL_THREE_PATHS))
            .expect("real RouterOS output");

        assert_eq!(
            result.paths.len(),
            3,
            "a route the router has must never be reported as absent"
        );
    }

    /// The legend is not followed by a blank line, so it arrives glued to the
    /// first entry. Skipping the block that starts with `Flags:` threw that
    /// route away with it.
    #[test]
    fn the_route_glued_to_the_legend_is_not_lost() {
        let driver = MikrotikDriver;
        let result = driver
            .parse_bgp_route(&as_routeros_sends_it(REAL_THREE_PATHS))
            .expect("real RouterOS output");

        let first = &result.paths[0];
        assert_eq!(
            first.next_hop.map(|h| h.to_string()).as_deref(),
            Some("192.0.2.6")
        );
        assert_eq!(first.med, Some(100));
    }

    /// Absent, zero and set, in one answer. A parser that prints 0 for a MED
    /// nobody sent is describing a route that does not exist (ADR-0006).
    #[test]
    fn an_absent_med_is_not_a_med_of_zero() {
        let driver = MikrotikDriver;
        let result = driver
            .parse_bgp_route(&as_routeros_sends_it(REAL_THREE_PATHS))
            .expect("real RouterOS output");

        let meds: Vec<Option<u32>> = result.paths.iter().map(|p| p.med).collect();
        assert_eq!(meds, vec![Some(100), None, Some(0)]);
    }

    /// RouterOS 7.16 prints the flags first with no index. Reading the second
    /// token as the flags found none, so no path was ever marked best.
    #[test]
    fn the_active_path_is_the_one_flagged_active() {
        let driver = MikrotikDriver;
        let result = driver
            .parse_bgp_route(&as_routeros_sends_it(REAL_THREE_PATHS))
            .expect("real RouterOS output");

        let best: Vec<bool> = result.paths.iter().map(|p| p.is_best).collect();
        assert_eq!(best, vec![false, false, true], "only `Ab` is active");
        assert_eq!(
            result
                .best()
                .and_then(|p| p.next_hop)
                .map(|h| h.to_string()),
            Some("192.0.2.10".to_string())
        );
    }

    /// The attributes are sub-properties of `bgp.`, printed with a leading dot
    /// on every line after the first. Matching `as-path=` alone read none.
    #[test]
    fn dotted_attributes_are_read() {
        let driver = MikrotikDriver;
        let result = driver
            .parse_bgp_route(&as_routeros_sends_it(REAL_THREE_PATHS))
            .expect("real RouterOS output");

        assert_eq!(
            result.paths[1].as_path,
            vec![64496, 64496, 64496, 65536, 65537, 65538, 65539, 65540, 65541, 64498],
            "a 32-bit ASN is one number, and the path survives its own line"
        );
        assert_eq!(result.paths[1].origin, Some(Origin::Igp));
    }

    /// RouterOS prints an AS_SET with no closing brace — its own output, not a
    /// truncation. The numbers inside are a set, so appending them to the
    /// sequence would claim an order the route never had.
    #[test]
    fn an_as_set_stops_the_path_and_marks_the_result_partial() {
        let raw = as_routeros_sends_it(
            r#"Flags: X - disabled, F - filtered, U - unreachable, A - active; 
H - hw-offloaded; + - ecmp, B - blackhole 
 Ab   afi=ip4 contribution=active dst-address=198.51.100.0/24 
       routing-table=main gateway=192.0.2.10 immediate-gw=192.0.2.10%ether4 
       bgp.peer-cache-id=*2800004 .aggregator="64498:192.0.2.10" 
       .as-path="64498{64496,64497" .med=0 .origin=igp 
"#,
        );

        let result = MikrotikDriver
            .parse_bgp_route(&raw)
            .expect("real RouterOS output");

        assert_eq!(result.paths.len(), 1);
        assert_eq!(
            result.paths[0].as_path,
            vec![64498],
            "the sequence stops at the set; its members are not a sequence"
        );
        assert!(
            matches!(result.completeness, Completeness::Partial { .. }),
            "a set the model cannot hold yet makes the reading partial, not wrong"
        );
    }

    /// RouterOS redraws the table as it discovers hops. The first draft shows
    /// hop 1 as `0ms` before the probe has timed out; the final one shows it as
    /// `timeout`. Reading the draft reported a round trip for a hop that never
    /// answered.
    #[test]
    fn a_hop_that_timed_out_keeps_its_number_and_carries_no_timing() {
        let raw = as_routeros_sends_it(
            r#"Columns: LOSS, SENT, LAST
#  LOSS  SENT  LAST
1  0%       1  0ms 

Columns: ADDRESS, LOSS, SENT, LAST, AVG, BEST, WORST, STD-DEV
#  ADDRESS       LOSS  SENT  LAST     AVG  BEST  WORST  STD-DEV
1                100%     1  timeout                           
2  203.0.113.10  0%       1  0.3ms    0.3  0.3   0.3          0
"#,
        );

        let result = MikrotikDriver
            .parse_traceroute(&raw)
            .expect("real RouterOS output");

        assert_eq!(result.hops.len(), 2, "a silent hop is still a hop");
        assert_eq!(result.hops[0].hop, 1);
        assert_eq!(result.hops[0].ip, None);
        assert!(
            result.hops[0].rtt_ms.is_empty(),
            "a hop that did not answer has no round trip time, not one of zero"
        );
        assert_eq!(result.hops[1].ip.as_deref(), Some("203.0.113.10"));
        assert_eq!(result.hops[1].rtt_ms, vec![0.3]);
    }

    /// Microseconds, which is what RouterOS reports on a link this short.
    #[test]
    fn a_ping_in_microseconds_is_not_read_as_milliseconds() {
        let raw = as_routeros_sends_it(
            r#"  SEQ HOST                                     SIZE TTL TIME       STATUS      
    0 203.0.113.10                               56  63 311us     
    1 203.0.113.10                               56  63 338us     
    sent=4 received=4 packet-loss=0% min-rtt=311us avg-rtt=358us 
   max-rtt=397us 
"#,
        );

        let result = MikrotikDriver
            .parse_ping(&raw)
            .expect("real RouterOS output");

        assert_eq!(result.packets_sent, 4);
        assert_eq!(result.packets_received, 4);
        assert_eq!(result.min_rtt_ms, Some(0.311));
        assert_eq!(result.avg_rtt_ms, Some(0.358));
        assert_eq!(
            result.max_rtt_ms,
            Some(0.397),
            "the summary wraps onto a second line and the value is still read"
        );
    }

    /// Captured from the same router. `/routing/bgp/session/print detail` is
    /// not a table, and the shared table reader found no sessions at all on a
    /// router with six.
    const REAL_SESSIONS: &str = r#"Flags: E - established 
 0 E name="peer-a-v6-1" 
     remote.address=2001:db8:0:1::2 .as=64496 .id=192.0.2.2 
     .capabilities=mp,rr,em,gr,as4,ap,err,llgr,fqdn .afi=ipv6 .messages=7 
     local.address=2001:db8:0:1::1 .as=64499 .id=192.0.2.1 
     .cluster-id=192.0.2.1 .capabilities=mp,rr,gr,as4 .afi=ipv6 .messages=7 
     output.procid=21 .filter-chain=BGP-OUT 
     input.procid=21 ebgp 
     hold-time=3m keepalive-time=1m uptime=4m48s290ms 
     last-started=2026-09-20 23:13:07 prefix-count=2 

 1 E name="peer-a-1" 
     remote.address=192.0.2.2 .as=64496 .id=192.0.2.2 
     .capabilities=mp,rr,em,gr,as4,ap,err,llgr,fqdn .afi=ip .messages=10 
     local.address=192.0.2.1 .as=64499 .id=192.0.2.1 .cluster-id=192.0.2.1 
     output.procid=22 .filter-chain=BGP-OUT 
     input.procid=22 ebgp 
     hold-time=3m keepalive-time=1m uptime=4m48s290ms 
     last-started=2026-09-20 23:13:07 prefix-count=4 
"#;

    #[test]
    fn the_sessions_are_read() {
        let result = MikrotikDriver
            .parse_bgp_summary(&as_routeros_sends_it(REAL_SESSIONS))
            .expect("real RouterOS output");

        assert_eq!(result.peers.len(), 2);
        assert_eq!(result.peers[0].peer_ip, "2001:db8:0:1::2");
        assert_eq!(result.peers[0].prefixes_received, 2);
        assert_eq!(result.peers[1].peer_ip, "192.0.2.2");
        assert_eq!(result.peers[1].prefixes_received, 4);
        assert_eq!(result.peers[1].uptime, "4m48s290ms");
    }

    /// `.as=` belongs to whichever of `remote` or `local` was named last.
    /// Reading it without tracking that gives every session this router's own
    /// AS, which looks plausible and is never right.
    #[test]
    fn the_peer_as_is_the_remote_one() {
        let result = MikrotikDriver
            .parse_bgp_summary(&as_routeros_sends_it(REAL_SESSIONS))
            .expect("real RouterOS output");

        assert_eq!(result.peers[0].peer_as, 64496, "the peer's AS, not ours");
        assert_eq!(result.local_as, Some(64499));
        assert_eq!(result.router_id.as_deref(), Some("192.0.2.1"));
    }

    /// `E` is the only flag RouterOS sets for a session that is up.
    #[test]
    fn a_session_without_the_established_flag_is_not_established() {
        let raw = as_routeros_sends_it(
            r#"Flags: E - established 
 0   name="never-1" 
     remote.address=192.0.2.126 .as=64511 
     local.address=192.0.2.1 .as=64499 
"#,
        );

        let result = MikrotikDriver
            .parse_bgp_summary(&raw)
            .expect("real RouterOS output");

        assert_eq!(result.peers.len(), 1);
        assert_eq!(result.peers[0].state, "Idle");
        assert_eq!(result.peers[0].prefixes_received, 0);
    }
}
