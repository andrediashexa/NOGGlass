use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, DriverError, Origin,
    PingResult, TracerouteResult, VendorDriver,
};

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
        // Shared reader: the tables differ in headers, not in what a row means.
        Ok(crate::summary::parse(raw))
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
}
