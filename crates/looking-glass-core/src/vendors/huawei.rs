use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, DriverError, Origin,
    PingResult, TracerouteResult, VendorDriver,
};
use ipnet::IpNet;

/// One route line, split into fields.
#[derive(Debug, Default)]
struct RouteFields {
    flags: String,
    network: String,
    next_hop: String,
    med: Option<u32>,
    local_pref: Option<u32>,
    pref_val: Option<u32>,
    path: String,
}

impl RouteFields {
    /// Splits a route line.
    ///
    /// Fields are found by what they are — flag characters, a network, an
    /// address — rather than by fixed column offsets, because VRP lets a wide
    /// value overflow its column and push everything right. That happens with
    /// IPv6 next hops routinely.
    ///
    /// `path_start` is the offset of the `Path/Ogn` header when the capture
    /// included one. It is the only thing the header is used for, and it is
    /// what disambiguates the numeric columns from the AS path: both are
    /// numbers, so nothing in the text itself separates them.
    fn from_line(line: &str, path_start: Option<usize>) -> Option<Self> {
        let tokens: Vec<(usize, &str)> = line
            .char_indices()
            .fold(Vec::new(), |mut acc: Vec<(usize, &str)>, (i, c)| {
                if c.is_whitespace() {
                    acc.push((usize::MAX, ""));
                } else if acc.last().map(|(o, _)| *o) == Some(usize::MAX) || acc.is_empty() {
                    acc.pop();
                    acc.push((i, &line[i..i + c.len_utf8()]));
                } else if let Some((o, _)) = acc.pop() {
                    acc.push((o, &line[o..i + c.len_utf8()]));
                }
                acc
            })
            .into_iter()
            .filter(|(o, t)| *o != usize::MAX && !t.is_empty())
            .collect();

        let mut fields = RouteFields::default();
        let mut rest = tokens.as_slice();

        // Status flags, when the build prints them.
        if let Some(((_, first), tail)) = rest.split_first() {
            if !first.is_empty() && first.chars().all(|c| "*>dhsi".contains(c)) {
                fields.flags = (*first).to_string();
                rest = tail;
            }
        }

        // Network, omitted on continuation lines.
        if let Some(((_, first), tail)) = rest.split_first() {
            if first.contains('/') && parse_network(first).is_some() {
                fields.network = (*first).to_string();
                rest = tail;
            }
        }

        // Next hop.
        if let Some(((_, first), tail)) = rest.split_first() {
            if first.parse::<std::net::IpAddr>().is_ok() {
                fields.next_hop = (*first).to_string();
                rest = tail;
            }
        }

        if fields.flags.is_empty() && fields.network.is_empty() && fields.next_hop.is_empty() {
            return None;
        }

        // Numeric columns versus AS path. With the header we know where the
        // path starts; without it, we keep the trailing tokens that carry an
        // origin marker and treat the leading numbers as metrics.
        let split_at = match path_start {
            Some(offset) => rest
                .iter()
                // Metrics are right-aligned, so a value may end just before the
                // Path column starts; the path itself begins at or after it.
                .position(|(o, _)| *o >= offset)
                .unwrap_or(rest.len()),
            None => rest
                .iter()
                .position(|(_, t)| {
                    t.chars()
                        .last()
                        .and_then(Origin::from_marker)
                        .is_some_and(|_| t.len() > 1)
                })
                .map_or(rest.len(), |marker_at| {
                    // The AS path runs from the first number after the metrics
                    // up to the marked token; VRP prints at most three metrics.
                    marker_at.saturating_sub(0).min(rest.len())
                }),
        };

        let mut split_at = split_at.min(rest.len());

        // VRP prints at most three numeric columns, and only numbers appear
        // there. Anything beyond that on the left of the split means the split
        // landed inside the AS path, so the boundary moves right.
        let numeric_prefix = rest[..split_at]
            .iter()
            .filter(|(_, t)| t.parse::<u32>().is_ok())
            .count();
        if numeric_prefix > 3 {
            split_at -= numeric_prefix - 3;
        }

        let (metrics, path) = rest.split_at(split_at);

        // Metrics are assigned right to left, because PrefVal is always printed
        // while MED and LocPrf may be blank. Reading them left to right is what
        // made an empty MED shift LocPrf into it.
        let numbers: Vec<u32> = metrics
            .iter()
            .filter_map(|(_, t)| t.parse::<u32>().ok())
            .collect();
        let mut from_right = numbers.iter().rev();
        fields.pref_val = from_right.next().copied();
        fields.local_pref = from_right.next().copied();
        fields.med = from_right.next().copied();

        fields.path = path.iter().map(|(_, t)| *t).collect::<Vec<_>>().join(" ");

        Some(fields)
    }
}

/// Reads the AS path and the trailing origin marker.
///
/// An absent marker leaves the origin `None`: the router did not say.
fn parse_as_path(text: &str) -> (Vec<u32>, Option<Origin>) {
    let mut as_path = Vec::new();
    let mut origin = None;
    for token in text.split_whitespace() {
        let last = token.chars().last();
        let marker = last.and_then(Origin::from_marker);
        let digits = match marker {
            Some(_) => &token[..token.len() - last.map_or(0, char::len_utf8)],
            None => token,
        };
        if let Some(m) = marker {
            origin = Some(m);
        }
        if let Ok(asn) = digits.parse::<u32>() {
            as_path.push(asn);
        }
    }
    (as_path, origin)
}

/// Huawei VRP: NE40E, NE8000 and the rest of the VRP family.
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

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
    }

    /// Parser de Texto BGP do Huawei VRP (Extrai Best Path '*' e '>' + Next-Hop + MED + LocPrf + AS-Path)
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // `display bgp routing-table` prints fixed-width columns under a header:
        //
        //  Status codes: * - valid, > - best, d - damped ...
        //    Network            NextHop         MED  LocPrf  PrefVal Path/Ogn
        // *>  198.51.100.0/24    192.0.2.254      10     150        0 65100 65500i
        // *                      198.51.100.254          100        0 65200 65500i
        //
        // Reading fields by their index among whitespace-separated tokens
        // breaks the moment a column is empty, which is routine for MED: every
        // later field shifts left, so LocPrf was read as MED and the AS path
        // lost its first hop — silently, which is the worst way to be wrong.
        //
        // The header gives the column offsets, so each line is sliced by
        // position instead. When the header is missing, which happens with a
        // truncated capture, the line falls back to the regex below and the
        // result is flagged partial rather than guessed.
        let mut paths = Vec::new();
        let mut unreadable = 0usize;
        let mut current_network: Option<IpNet> = None;
        // Offset of the Path/Ogn column, taken from the header when present.
        let mut path_start: Option<usize> = None;

        for line in raw.lines() {
            let body = line.trim_end();
            let header = body.trim_start();

            if header.starts_with("Network") {
                path_start = body.find("Path/Ogn").or_else(|| body.find("Path"));
                continue;
            }
            if header.is_empty()
                || header.starts_with("Total")
                || header.starts_with("BGP")
                || header.starts_with("Status")
                || header.starts_with("Route Flag")
                || header.starts_with("Paths:")
                || header.starts_with("VPN-Instance")
            {
                continue;
            }

            let Some(fields) = RouteFields::from_line(body, path_start) else {
                unreadable += 1;
                continue;
            };

            if !fields.network.is_empty() {
                current_network = parse_network(&fields.network);
            }
            let next_hop = parse_hop(&fields.next_hop);
            if current_network.is_none() && next_hop.is_none() {
                unreadable += 1;
                continue;
            }

            let (as_path, origin) = parse_as_path(&fields.path);

            paths.push(BgpPath {
                prefix: current_network,
                next_hop,
                is_best: fields.flags.contains('>'),
                is_valid: Some(fields.flags.contains('*')),
                as_path,
                med: fields.med,
                local_pref: fields.local_pref,
                weight: fields.pref_val,
                origin,
                ..BgpPath::default()
            });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Completeness;

    /// Real-world VRP output: the MED column is empty on the second path, and
    /// the third has neither MED nor LocPrf. Reading fields positionally made
    /// LocPrf land in MED and dropped the first AS of the path.
    #[test]
    fn empty_columns_do_not_shift_the_other_fields() {
        let raw = "\
 BGP Local router ID is 192.0.2.1
 Status codes: * - valid, > - best, d - damped, h - history, i - internal
 Total Number of Routes: 3
   Network            NextHop         MED   LocPrf  PrefVal Path/Ogn
*>  198.51.100.0/24    192.0.2.254      10      150        0 65100 65500i
*                      192.0.2.253              120        0 65200 65500i
*                      192.0.2.252                         0 65300 65500i
";
        let result = HuaweiVrpDriver
            .parse_bgp_route(raw)
            .expect("VRP table should parse");
        assert_eq!(result.completeness, Completeness::Complete);
        assert_eq!(result.paths.len(), 3);

        let best = result.best().expect("the > flag marks the best path");
        assert_eq!(best.med, Some(10));
        assert_eq!(best.local_pref, Some(150));
        assert_eq!(best.as_path, vec![65100, 65500]);
        assert_eq!(best.origin, Some(Origin::Igp));

        // MED absent: LocPrf must stay in LocPrf, and the AS path must keep
        // both hops.
        let second = &result.paths[1];
        assert_eq!(second.med, None, "MED column was empty");
        assert_eq!(second.local_pref, Some(120));
        assert_eq!(second.as_path, vec![65200, 65500]);
        assert!(!second.is_best);

        // Both numeric columns absent.
        let third = &result.paths[2];
        assert_eq!(third.med, None);
        assert_eq!(third.local_pref, None);
        assert_eq!(third.as_path, vec![65300, 65500]);
    }

    /// Continuation lines inherit the prefix of the line above.
    #[test]
    fn continuation_lines_keep_the_prefix() {
        let raw = "\
   Network            NextHop         MED   LocPrf  PrefVal Path/Ogn
*>  198.51.100.0/24    192.0.2.254      10      150        0 65100i
*                      192.0.2.253      20      100        0 65200i
";
        let result = HuaweiVrpDriver.parse_bgp_route(raw).unwrap();
        assert_eq!(result.paths.len(), 2);
        for path in &result.paths {
            assert_eq!(
                path.prefix.unwrap().to_string(),
                "198.51.100.0/24",
                "both paths are for the same prefix"
            );
        }
        assert_eq!(result.paths[1].next_hop.unwrap().to_string(), "192.0.2.253");
    }

    #[test]
    fn parses_ipv6_tables() {
        let raw = "\
   Network            NextHop         MED   LocPrf  PrefVal Path/Ogn
*>  2001:db8::/32      2001:db8:ffff::1  0      150        0 65100 65500e
";
        let result = HuaweiVrpDriver.parse_bgp_route(raw).unwrap();
        let path = &result.paths[0];
        assert_eq!(path.prefix.unwrap().to_string(), "2001:db8::/32");
        assert_eq!(
            path.next_hop.unwrap().to_string(),
            "2001:db8:ffff::1",
            "IPv6 next hop"
        );
        assert_eq!(path.origin, Some(Origin::Egp));
    }

    /// Output captured without its header still parses, and anything the
    /// fallback cannot read is reported rather than dropped.
    #[test]
    fn output_without_a_header_falls_back_and_reports_what_it_cannot_read() {
        let raw = "\
*>  198.51.100.0/24    192.0.2.254      10      150        0 65100 65500i
this line is not a route at all
";
        let result = HuaweiVrpDriver.parse_bgp_route(raw).unwrap();
        assert_eq!(result.paths.len(), 1);
        assert_eq!(
            result.paths[0].prefix.unwrap().to_string(),
            "198.51.100.0/24"
        );
        assert!(
            matches!(result.completeness, Completeness::Partial { .. }),
            "got {:?}",
            result.completeness
        );
        assert_eq!(result.raw_output, raw, "raw output is always preserved");
    }

    #[test]
    fn a_table_with_no_routes_is_a_complete_answer() {
        let raw = " Total Number of Routes: 0\n";
        let result = HuaweiVrpDriver.parse_bgp_route(raw).unwrap();
        assert!(result.paths.is_empty());
        assert_eq!(result.completeness, Completeness::Complete);
    }
}
