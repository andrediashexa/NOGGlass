use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, Community, DriverError,
    Origin, PingResult, TracerouteResult, VendorDriver,
};

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

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        // IOS and IOS-XR both end a ping with one summary line:
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
                    if let Some(rate) = rate_str
                        .split_whitespace()
                        .last()
                        .and_then(|s| s.parse::<f64>().ok())
                    {
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
            }
            // The round-trip figures are on the same line as the success rate,
            // so this cannot be an `else`: it used to be, and every Cisco ping
            // came back without timings.
            if line.contains("round-trip min/avg/max") {
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
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
    }

    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // Two shapes, and which one arrives depends on the command. Asking
        // about one prefix — what the catalogue sends for a route query —
        // answers in blocks:
        //
        //   BGP routing table entry for 203.0.113.0/24, version 7
        //   Paths: (3 available, best #1, table default)
        //     Refresh Epoch 1
        //     64498
        //       192.0.2.10 from 192.0.2.10 (192.0.2.10)
        //         Origin IGP, metric 0, localpref 100, valid, external, best
        //
        // The columns `bgp_table` reads come from the argument-less command
        // and from the AS-path query, so both are needed. Reading only the
        // columns gave three paths with no prefix, no next hop and an AS path
        // of [0] — a wrong answer rather than an error.
        if raw.contains("BGP routing table entry for") {
            return Ok(parse_detail_blocks(raw));
        }

        // IOS-XE and IOS-XR print the table `bgp_table` reads; so does DmOS.
        Ok(crate::bgp_table::parse(raw))
    }

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        // Shared reader: the tables differ in headers, not in what a row means.
        Ok(crate::summary::parse(raw))
    }
}

/// Reads IOS's per-path detail blocks.
///
/// The anchor is the `<next hop> from <peer> (<router id>)` line: the AS path
/// is the line above it, the attributes are the lines below. Anchoring on
/// `Refresh Epoch` instead would work on IOS-XE and not on platforms that do
/// not print it.
pub(crate) fn parse_detail_blocks(raw: &str) -> BgpRouteResult {
    let mut paths: Vec<BgpPath> = Vec::new();
    let mut prefix = None;
    let mut pending_as_path: Option<(Vec<u32>, bool)> = None;
    let mut partial = 0usize;

    for line in raw.lines() {
        let line = line.trim();

        if let Some(rest) = line.strip_prefix("BGP routing table entry for") {
            // `for 203.0.113.0/24, version 7`
            prefix = rest
                .split(',')
                .next()
                .and_then(|text| parse_network(text.trim()));
            continue;
        }

        // `192.0.2.10 from 192.0.2.10 (192.0.2.10)` opens a path.
        if let Some((next_hop, rest)) = line.split_once(" from ") {
            if let Some(next_hop) = parse_hop(next_hop.trim()) {
                let (as_path, had_set) = pending_as_path.take().unwrap_or_default();
                if had_set {
                    partial += 1;
                }
                paths.push(BgpPath {
                    prefix,
                    next_hop: Some(next_hop),
                    peer: rest.split_whitespace().next().and_then(parse_hop),
                    as_path,
                    ..BgpPath::default()
                });
                continue;
            }
        }

        if let Some(path) = paths.last_mut() {
            if line.starts_with("Origin ") {
                read_attributes(line, path);
                continue;
            }
            if let Some(rest) = line.strip_prefix("Community:") {
                path.communities
                    .extend(rest.split_whitespace().map(Community::parse));
                continue;
            }
            if let Some(rest) = line.strip_prefix("Updated on ") {
                path.age = Some(rest.trim().to_string());
                continue;
            }
            if let Some(rest) = line.strip_prefix("Last update: ") {
                path.age = Some(rest.trim().to_string());
                continue;
            }
        }

        // An AS path sits on its own line just above the next hop. `Local`
        // means this router originated the route, which is an empty path
        // rather than an unreadable one.
        if let Some(as_path) = read_as_path_line(line) {
            pending_as_path = Some(as_path);
        }
    }

    BgpRouteResult::new(paths, raw).partial(partial)
}

/// `Origin IGP, metric 0, localpref 100, valid, external, best`
fn read_attributes(line: &str, path: &mut BgpPath) {
    for attribute in line.split(',') {
        let attribute = attribute.trim();
        let mut words = attribute.split_whitespace();
        match (words.next(), words.next()) {
            (Some("Origin"), Some(origin)) => {
                path.origin = match origin.to_ascii_lowercase().as_str() {
                    "igp" => Some(Origin::Igp),
                    "egp" => Some(Origin::Egp),
                    "incomplete" => Some(Origin::Incomplete),
                    _ => None,
                }
            }
            // Absent is not zero: a path with no MED has no `metric` here and
            // keeps None (ADR-0006).
            (Some("metric"), Some(value)) => path.med = value.parse().ok(),
            (Some("localpref"), Some(value)) => path.local_pref = value.parse().ok(),
            (Some("weight"), Some(value)) => path.weight = value.parse().ok(),
            (Some("valid"), _) => path.is_valid = Some(true),
            (Some("best"), _) => path.is_best = true,
            _ => {}
        }
    }
}

/// Reads a line that carries nothing but an AS path.
///
/// Returns the sequence and whether it ended at an AS_SET, whose members are a
/// set rather than a sequence and are not appended to it.
fn read_as_path_line(line: &str) -> Option<(Vec<u32>, bool)> {
    if line.is_empty() {
        return None;
    }
    if line.eq_ignore_ascii_case("Local") {
        return Some((Vec::new(), false));
    }

    let mut sequence = Vec::new();
    for token in line.split_whitespace() {
        if token.starts_with('{') {
            return Some((sequence, true));
        }
        match token.parse::<u32>() {
            Ok(asn) => sequence.push(asn),
            // Any other word means this is not an AS path line.
            Err(_) => return None,
        }
    }

    if sequence.is_empty() {
        None
    } else {
        Some((sequence, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{Completeness, Origin};

    /// IOS-XE tabular output. The status flags are glued to the network, and
    /// the numeric columns are right-aligned, so a whitespace split reads them
    /// in the wrong places.
    #[test]
    fn parses_ios_xe_tables_with_attributes() {
        let raw = "\
BGP table version is 42, local router ID is 192.0.2.1
Status codes: s suppressed, d damped, h history, * valid, > best, i - internal
Origin codes: i - IGP, e - EGP, ? - incomplete

     Network          Next Hop            Metric LocPrf Weight Path
 *>  198.51.100.0/24  192.0.2.254             10    150      0 65100 65500 i
 *                   192.0.2.253             20    100      0 65200 65500 i
";
        let result = CiscoDriver::new(false)
            .parse_bgp_route(raw)
            .expect("IOS-XE output should parse");

        assert_eq!(result.completeness, Completeness::Complete);
        assert_eq!(result.paths.len(), 2);

        let best = result.best().expect("the > flag marks the best path");
        assert_eq!(best.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(best.next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(
            best.as_path,
            vec![65100, 65500],
            "the AS path was never populated before"
        );
        assert_eq!(best.med, Some(10));
        assert_eq!(best.local_pref, Some(150));
        assert_eq!(best.weight, Some(0));
        assert_eq!(best.origin, Some(Origin::Igp));
        assert_eq!(best.origin_as(), Some(65500));

        // The second line omits the network: it belongs to the prefix above.
        let alternative = &result.paths[1];
        assert_eq!(
            alternative.prefix.unwrap().to_string(),
            "198.51.100.0/24",
            "a continuation line keeps the prefix"
        );
        assert_eq!(alternative.next_hop.unwrap().to_string(), "192.0.2.253");
        assert!(!alternative.is_best);
    }

    /// IOS-XR prints the same table. The driver used to ask for JSON and then
    /// parse text, so every XR query came back empty.
    #[test]
    fn parses_ios_xr_tables() {
        // A raw string, and deliberately: `"\` eats the leading whitespace of
        // the next line, so the header lost the five spaces that line its
        // columns up with the row. The fixture then described a table no
        // router prints, and the reader that uses the header's offsets had no
        // way to be right about it.
        let raw = r#"     Network          Next Hop            Metric LocPrf Weight Path
 *>i 198.51.100.0/24  192.0.2.254              0    150      0 65100 65500 i
"#;
        let result = CiscoDriver::new(true)
            .parse_bgp_route(raw)
            .expect("IOS-XR output should parse");

        assert_eq!(result.paths.len(), 1, "XR used to return nothing at all");
        assert_eq!(result.paths[0].as_path, vec![65100, 65500]);
        assert!(result.paths[0].is_best);
    }

    #[test]
    fn the_catalogue_no_longer_asks_cisco_for_json() {
        for vendor in ["cisco_iosxe", "cisco_iosxr"] {
            let commands = crate::catalogue::BUILTIN.vendor(vendor).unwrap();
            for template in [&commands.bgp_route_v4, &commands.bgp_route_v6] {
                let template = template.as_ref().expect("Cisco answers BGP queries");
                assert!(
                    !template.contains("json"),
                    "{vendor} asks for JSON but the driver parses text: {template}"
                );
            }
        }
    }

    #[test]
    fn a_table_with_no_routes_is_a_complete_answer() {
        let raw = "% Network not in table\n";
        let result = CiscoDriver::new(false).parse_bgp_route(raw).unwrap();
        assert!(result.paths.is_empty());
        assert_eq!(result.raw_output, raw);
    }

    #[test]
    fn ping_reports_what_the_router_said() {
        let raw = "\
Type escape sequence to abort.
Sending 5, 100-byte ICMP Echos to 198.51.100.1, timeout is 2 seconds:
!!!!!
Success rate is 100 percent (5/5), round-trip min/avg/max = 1/2/4 ms
";
        let result = CiscoDriver::new(false).parse_ping(raw).unwrap();
        assert_eq!(result.packets_sent, 5);
        assert_eq!(result.packets_received, 5);
        assert_eq!(result.packet_loss_percent, 0.0);
        assert_eq!(result.avg_rtt_ms, Some(2.0));
    }

    /// A failed ping must read as failed, not as an absent result.
    #[test]
    fn a_ping_that_lost_everything_says_so() {
        let raw = "Success rate is 0 percent (0/5)\n";
        let result = CiscoDriver::new(false).parse_ping(raw).unwrap();
        assert_eq!(result.packets_received, 0);
        assert_eq!(result.packet_loss_percent, 100.0);
        assert_eq!(result.avg_rtt_ms, None);
    }
}

#[cfg(test)]
mod real_iosxe_tests {
    use super::*;
    use crate::driver::Completeness;

    /// Captured from a CSR1000v running IOS-XE 17.03.08a in `lab/`, peering
    /// with three FRR speakers. This is the answer to the command the
    /// catalogue sends, which is not the shape the column reader was written
    /// for.
    const REAL_DETAIL: &str = r#"BGP routing table entry for 203.0.113.0/24, version 7
Paths: (3 available, best #1, table default)
  Not advertised to any peer
  Refresh Epoch 1
  64498
    192.0.2.10 from 192.0.2.10 (192.0.2.10)
      Origin IGP, metric 0, localpref 100, valid, external, best
      rx pathid: 0, tx pathid: 0x0
      Updated on Sep 21 2026 06:56:50 UTC
  Refresh Epoch 1
  64497 64498
    192.0.2.6 from 192.0.2.6 (192.0.2.6)
      Origin IGP, metric 100, localpref 100, valid, external
      rx pathid: 0, tx pathid: 0
      Updated on Sep 21 2026 06:56:40 UTC
  Refresh Epoch 1
  64496 64496 64496 65536 65537 65538 65539 65540 65541 64498
    192.0.2.2 from 192.0.2.2 (192.0.2.2)
      Origin IGP, localpref 100, valid, external
      rx pathid: 0, tx pathid: 0
      Updated on Sep 21 2026 06:56:38 UTC
"#;

    /// Before this, the table reader produced three paths with no prefix, no
    /// next hop and an AS path of [0].
    #[test]
    fn the_detail_format_is_read() {
        let result = CiscoDriver::new(false)
            .parse_bgp_route(REAL_DETAIL)
            .expect("real IOS-XE output");

        assert_eq!(result.paths.len(), 3);
        assert_eq!(
            result.paths[0].age.as_deref(),
            Some("Sep 21 2026 06:56:50 UTC"),
            "route update timestamp is parsed as age"
        );
        for path in &result.paths {
            assert_eq!(
                path.prefix.map(|p| p.to_string()).as_deref(),
                Some("203.0.113.0/24")
            );
            assert!(path.next_hop.is_some(), "every path has a next hop");
        }
    }

    /// Absent, zero and set, in one answer (ADR-0006).
    #[test]
    fn an_absent_med_is_not_a_med_of_zero() {
        let result = CiscoDriver::new(false)
            .parse_bgp_route(REAL_DETAIL)
            .expect("real IOS-XE output");

        let meds: Vec<Option<u32>> = result.paths.iter().map(|p| p.med).collect();
        assert_eq!(meds, vec![Some(0), Some(100), None]);
    }

    /// `best` is a word on the attribute line here, not a column flag.
    #[test]
    fn the_best_path_is_the_one_marked_best() {
        let result = CiscoDriver::new(false)
            .parse_bgp_route(REAL_DETAIL)
            .expect("real IOS-XE output");

        let best: Vec<bool> = result.paths.iter().map(|p| p.is_best).collect();
        assert_eq!(best, vec![true, false, false]);
    }

    /// IOS-XE prints 32-bit ASNs as plain numbers, where VRP uses asdot.
    #[test]
    fn a_long_path_with_32_bit_asns_is_read_whole() {
        let result = CiscoDriver::new(false)
            .parse_bgp_route(REAL_DETAIL)
            .expect("real IOS-XE output");

        assert_eq!(
            result.paths[2].as_path,
            vec![64496, 64496, 64496, 65536, 65537, 65538, 65539, 65540, 65541, 64498]
        );
    }

    /// The set's members are not a sequence, and the aggregator note after it
    /// is not an AS.
    #[test]
    fn an_as_set_stops_the_path_and_marks_the_result_partial() {
        let raw = r#"BGP routing table entry for 198.51.100.0/24, version 4
Paths: (1 available, best #1, table default)
  Refresh Epoch 1
  64498 {64496,64497}, (aggregated by 64498 192.0.2.10)
    192.0.2.10 from 192.0.2.10 (192.0.2.10)
      Origin IGP, metric 0, localpref 100, valid, external, best
"#;

        let result = CiscoDriver::new(false)
            .parse_bgp_route(raw)
            .expect("real IOS-XE output");

        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].as_path, vec![64498]);
        assert!(matches!(result.completeness, Completeness::Partial { .. }));
    }

    /// The AS-path query still answers in columns, and that reader is still
    /// the one that has to handle it. Captured from the same router.
    #[test]
    fn the_column_format_still_works() {
        let raw = "\
BGP table version is 7, local router ID is 192.0.2.1
Status codes: s suppressed, d damped, h history, * valid, > best, i - internal, 
              r RIB-failure, S Stale, m multipath, b backup-path, f RT-Filter, 
Origin codes: i - IGP, e - EGP, ? - incomplete
RPKI validation codes: V valid, I invalid, N Not found

     Network          Next Hop            Metric LocPrf Weight Path
 *    192.0.2.128/25   192.0.2.2                              0 64496 65536 64498 64497 i
 *>   198.51.100.0/25  192.0.2.2                0             0 64496 65536 i
 *>   198.51.100.42/32 192.0.2.2                0             0 64496 65536 i
 *    203.0.113.0      192.0.2.2                              0 64496 64496 64496 65536 65537 65538 65539 65540 65541 64498 i
";
        let result = CiscoDriver::new(false)
            .parse_bgp_route(raw)
            .expect("real IOS-XE columns");

        assert_eq!(result.paths.len(), 4);
        assert_eq!(result.paths[1].as_path, vec![64496, 65536]);
        // The first row has no metric at all and the second has 0, on the same
        // screen. A reader that splits on whitespace shifts every later field
        // when a column is empty.
        assert_eq!(result.paths[0].med, None);
        assert_eq!(result.paths[1].med, Some(0));
    }
}
