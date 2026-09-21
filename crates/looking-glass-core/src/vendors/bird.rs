use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpPeerSummary, BgpRouteResult, BgpSummaryResult, Community,
    DriverError, Origin, PingResult, TracerouteResult, VendorDriver,
};

pub struct BirdDriver;

impl VendorDriver for BirdDriver {
    fn vendor_name(&self) -> &'static str {
        "bird_routing_daemon"
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        // BIRD runs on a Linux host, so this is iputils output.
        crate::ping::parse(raw)
    }

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
    }

    /// Reads BIRD's `show route for <prefix> all`.
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // BIRD 2 prints one route as a header line plus indented attributes:
        //
        // 198.51.100.0/24 unicast [upstream1 12:00:00] * (100) [AS65500i]
        //     via 192.0.2.254 on eth0
        //     BGP.as_path: 65100 65500
        //     BGP.local_pref: 150
        //     BGP.community: (65001,100) (65100,500)
        //
        // Every attribute belongs to the header above it, so the accumulator is
        // replaced wholesale on each header. Clearing only some fields used to
        // leak the previous route's next hop and metrics into the next one.
        let mut paths: Vec<BgpPath> = Vec::new();
        let mut current: Option<BgpPath> = None;
        // BIRD prints the prefix once and leaves the column blank on every
        // path that follows. Blank does not mean "no prefix", it means "the
        // same one", and a path with no prefix is not something the interface
        // can draw.
        let mut last_prefix = None;

        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if line.contains("unicast") || line.contains("unreachable") {
                if let Some(path) = current.take() {
                    paths.push(path);
                }
                let mut path = BgpPath {
                    is_best: line.contains('*'),
                    ..BgpPath::default()
                };
                // `unicast` as the first token means the prefix column was
                // blank: this path belongs to the prefix above it.
                match line.split_whitespace().next() {
                    Some(first)
                        if first.starts_with("unicast") || first.starts_with("unreachable") =>
                    {
                        path.prefix = last_prefix;
                    }
                    Some(prefix) => {
                        path.prefix = parse_network(prefix);
                        last_prefix = path.prefix;
                    }
                    None => {}
                }
                // The trailing [AS65500i] carries the origin marker.
                if let Some(tail) = line.rsplit('[').next() {
                    let tail = tail.trim_end_matches(']');
                    if let Some(marker) = tail.chars().last() {
                        path.origin = Origin::from_marker(marker);
                    }
                }
                current = Some(path);
                continue;
            }

            let Some(path) = current.as_mut() else {
                continue;
            };

            if let Some(rest) = line.strip_prefix("via ") {
                path.next_hop = rest.split_whitespace().next().and_then(parse_hop);
            } else if let Some(rest) = line.strip_prefix("BGP.as_path:") {
                path.as_path = rest
                    .split_whitespace()
                    .filter_map(|token| token.parse::<u32>().ok())
                    .collect();
            } else if let Some(rest) = line.strip_prefix("BGP.local_pref:") {
                path.local_pref = rest.trim().parse().ok();
            } else if let Some(rest) = line.strip_prefix("BGP.med:") {
                path.med = rest.trim().parse().ok();
            } else if let Some(rest) = line.strip_prefix("BGP.community:") {
                path.communities = rest.split_whitespace().map(Community::parse).collect();
            } else if let Some(rest) = line.strip_prefix("BGP.large_community:") {
                path.communities
                    .extend(rest.split_whitespace().map(Community::parse));
            } else if let Some(rest) = line.strip_prefix("BGP.next_hop:") {
                path.next_hop = rest.split_whitespace().next().and_then(parse_hop);
            }
        }

        if let Some(path) = current.take() {
            paths.push(path);
        }

        Ok(BgpRouteResult::new(paths, raw))
    }

    /// Reads BIRD's `show protocols all`.
    ///
    /// The shared reader is for table-shaped output, and this is not that:
    /// BIRD prints one line per protocol and then an indented block. Handing
    /// it to the table reader found no rows at all, so a router with six
    /// established sessions reported none.
    ///
    ///   peer_c     BGP        ---        up     00:44:09.180  Established
    ///     BGP state:          Established
    ///       Neighbor address: 192.0.2.10
    ///       Neighbor AS:      64498
    ///       Routes:         3 imported, 0 exported, 2 preferred
    ///       Route change stats:     received   rejected   filtered   ignored   accepted
    ///         Import updates:              3          0          0         0          3
    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        let mut peers: Vec<BgpPeerSummary> = Vec::new();
        let mut local_as = None;

        let mut current: Option<BgpPeerSummary> = None;

        for line in raw.lines() {
            let indented = line.starts_with(char::is_whitespace);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // A protocol starts at the left margin. Anything indented belongs
            // to the protocol above it.
            if !indented {
                if let Some(peer) = current.take() {
                    peers.push(peer);
                }
                let mut columns = trimmed.split_whitespace();
                let (Some(_name), Some(proto)) = (columns.next(), columns.next()) else {
                    continue;
                };
                if proto != "BGP" {
                    continue;
                }
                // Columns: table, state, since, and the info the state came
                // with. `since` is the uptime this project reports.
                let _table = columns.next();
                let _state = columns.next();
                let since = columns.next().unwrap_or("").to_string();

                current = Some(BgpPeerSummary {
                    peer_ip: String::new(),
                    peer_as: 0,
                    // Replaced by the block's own `BGP state:`, which names the
                    // session state rather than whether the protocol is running.
                    state: String::new(),
                    uptime: since,
                    prefixes_received: 0,
                    prefixes_accepted: None,
                });
                continue;
            }

            let Some(peer) = current.as_mut() else {
                // `Local AS` also appears inside a block, but a router-wide
                // value is worth keeping even if the first block is not BGP.
                continue;
            };

            if let Some(rest) = trimmed.strip_prefix("BGP state:") {
                peer.state = rest.trim().to_string();
            } else if let Some(rest) = trimmed.strip_prefix("Neighbor address:") {
                peer.peer_ip = rest.trim().to_string();
            } else if let Some(rest) = trimmed.strip_prefix("Neighbor AS:") {
                peer.peer_as = rest.trim().parse().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("Local AS:") {
                local_as = rest.trim().parse().ok();
            } else if let Some(rest) = trimmed.strip_prefix("Routes:") {
                // `3 imported, 0 filtered, 0 exported, 2 preferred`.
                //
                // Not the `Route change stats` block below it: those are
                // cumulative counters. A session that received four updates and
                // one withdrawal reads `4 received` there and holds three
                // routes, and reporting the counter as the prefix count would
                // show a number that only ever grows.
                //
                // `filtered` appears only when the router keeps filtered routes.
                // When it does, received is everything that arrived and
                // accepted is what survived the filter; when it does not, BIRD
                // reports one number and there is no second one to invent.
                let mut imported = None;
                let mut filtered = None;
                let mut previous: Option<&str> = None;
                for token in rest.split([',', ' ']).filter(|t| !t.is_empty()) {
                    match token {
                        "imported" => imported = previous.and_then(|n| n.parse::<u32>().ok()),
                        "filtered" => filtered = previous.and_then(|n| n.parse::<u32>().ok()),
                        _ => {}
                    }
                    previous = Some(token);
                }
                let imported = imported.unwrap_or(0);
                match filtered {
                    Some(filtered) => {
                        peer.prefixes_received = imported + filtered;
                        peer.prefixes_accepted = Some(imported);
                    }
                    None => peer.prefixes_received = imported,
                }
            }
        }

        if let Some(peer) = current.take() {
            peers.push(peer);
        }

        // A protocol block with no neighbour address is not a session: BIRD
        // prints device and kernel protocols in the same list.
        peers.retain(|peer| !peer.peer_ip.is_empty());

        Ok(BgpSummaryResult {
            router_id: None,
            local_as,
            peers,
            raw_output: raw.to_string(),
        })
    }
}

#[cfg(test)]
mod real_output_tests {
    use super::*;

    /// Captured from BIRD 2.15.1 in `lab/`, peering with three FRR speakers.
    /// BIRD prints the prefix once; the paths that follow leave the column
    /// blank and belong to the prefix above them.
    const REAL_THREE_PATHS: &str = r#"BIRD 2.15.1 ready.
Table master4:
203.0.113.0/24       unicast [peer_c 00:44:10.480] * (100) [AS64498i]
	via 192.0.2.10 on eth3
	Type: BGP univ
	BGP.origin: IGP
	BGP.as_path: 64498
	BGP.next_hop: 192.0.2.10
	BGP.med: 0
	BGP.local_pref: 100
                     unicast [peer_a 00:44:13.208] (100) [AS64498i]
	via 192.0.2.2 on eth1
	Type: BGP univ
	BGP.origin: IGP
	BGP.as_path: 64496 64496 64496 65536 65537 65538 65539 65540 65541 64498
	BGP.next_hop: 192.0.2.2
	BGP.local_pref: 100
                     unicast [peer_b 00:44:13.898] (100) [AS64498i]
	via 192.0.2.6 on eth2
	Type: BGP univ
	BGP.origin: IGP
	BGP.as_path: 64497 64498
	BGP.next_hop: 192.0.2.6
	BGP.med: 100
	BGP.local_pref: 100
"#;

    /// A blank prefix column means "the same prefix", not "no prefix". A path
    /// without one is not something the interface can draw.
    #[test]
    fn every_path_carries_the_prefix_even_when_the_column_is_blank() {
        let result = BirdDriver
            .parse_bgp_route(REAL_THREE_PATHS)
            .expect("real BIRD output");

        assert_eq!(result.paths.len(), 3);
        for path in &result.paths {
            assert_eq!(
                path.prefix.map(|p| p.to_string()).as_deref(),
                Some("203.0.113.0/24"),
                "a continuation path belongs to the prefix printed above it"
            );
        }
    }

    /// The first path is the one BIRD marked with `*`.
    #[test]
    fn the_starred_path_is_the_best_one() {
        let result = BirdDriver
            .parse_bgp_route(REAL_THREE_PATHS)
            .expect("real BIRD output");

        let best: Vec<bool> = result.paths.iter().map(|p| p.is_best).collect();
        assert_eq!(best, vec![true, false, false]);
    }

    /// Absent, zero and set, in one answer (ADR-0006).
    #[test]
    fn an_absent_med_is_not_a_med_of_zero() {
        let result = BirdDriver
            .parse_bgp_route(REAL_THREE_PATHS)
            .expect("real BIRD output");

        let meds: Vec<Option<u32>> = result.paths.iter().map(|p| p.med).collect();
        assert_eq!(meds, vec![Some(0), None, Some(100)]);
    }

    /// Ten ASNs, two of them 32-bit, on one line.
    #[test]
    fn a_long_path_with_32_bit_asns_is_read_whole() {
        let result = BirdDriver
            .parse_bgp_route(REAL_THREE_PATHS)
            .expect("real BIRD output");

        assert_eq!(
            result.paths[1].as_path,
            vec![64496, 64496, 64496, 65536, 65537, 65538, 65539, 65540, 65541, 64498]
        );
    }

    /// `show protocols all` is not table-shaped, and handing it to the shared
    /// table reader found no sessions at all on a router with six.
    const REAL_SUMMARY: &str = r#"BIRD 2.15.1 ready.
Name       Proto      Table      State  Since         Info
device1    Device     ---        up     00:44:08.620  

peer_b     BGP        ---        up     00:44:12.901  Established   
  BGP state:          Established
    Neighbor address: 192.0.2.6
    Neighbor AS:      64497
    Local AS:         64499
    Session:          external AS4
  Channel ipv4
    State:          UP
    Routes:         3 imported, 0 exported, 2 preferred
    Route change stats:     received   rejected   filtered    ignored   accepted
      Import updates:              4          0          0          0          4
      Import withdraws:            1          0        ---          0          1

never      BGP        ---        start  00:44:08.620  Idle          
  BGP state:          Idle
    Neighbor address: 192.0.2.126
    Neighbor AS:      64511
    Local AS:         64499
  Channel ipv4
    State:          DOWN
"#;

    #[test]
    fn the_summary_lists_every_session_including_the_one_that_is_down() {
        let result = BirdDriver
            .parse_bgp_summary(REAL_SUMMARY)
            .expect("real BIRD output");

        assert_eq!(result.local_as, Some(64499));
        assert_eq!(
            result.peers.len(),
            2,
            "the device protocol is not a session"
        );

        assert_eq!(result.peers[0].peer_ip, "192.0.2.6");
        assert_eq!(result.peers[0].peer_as, 64497);
        assert_eq!(result.peers[0].state, "Established");
        assert_eq!(result.peers[0].uptime, "00:44:12.901");

        let down = &result.peers[1];
        assert_eq!(down.peer_ip, "192.0.2.126");
        assert_eq!(down.state, "Idle");
        assert_eq!(down.prefixes_received, 0);
    }

    /// `Import updates` is cumulative: this session received four updates and
    /// one withdrawal, and holds three routes. Reporting the counter would show
    /// a prefix count that only ever grows.
    #[test]
    fn the_prefix_count_is_the_current_one_not_a_counter() {
        let result = BirdDriver
            .parse_bgp_summary(REAL_SUMMARY)
            .expect("real BIRD output");

        assert_eq!(result.peers[0].prefixes_received, 3);
        assert_eq!(
            result.peers[0].prefixes_accepted, None,
            "BIRD reports one number here; a second one would be invented"
        );
    }
}
