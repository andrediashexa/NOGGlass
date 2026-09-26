use crate::driver::{
    parse_hop, parse_network, BgpPath, BgpRouteResult, BgpSummaryResult, Community, DriverError,
    Origin, PingResult, RpkiStatus, RpkiValidation, TracerouteResult, VendorDriver,
};
use ipnet::IpNet;

/// One route line, split into fields.
#[derive(Debug, Default)]
struct RouteFields {
    flags: String,
    rpki: Option<RpkiStatus>,
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

        // RPKI validation status code, when VRP prints the RPKI column (V - valid, I - invalid, N - not-found).
        if let Some(((_, first), tail)) = rest.split_first() {
            let status = match *first {
                "V" => Some(RpkiStatus::Valid),
                "I" => Some(RpkiStatus::Invalid),
                "N" => Some(RpkiStatus::NotFound),
                _ => None,
            };
            if let Some(s) = status {
                fields.rpki = Some(s);
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
        // VRP prints the statistics in two shapes. The documented one puts
        // them on a single line:
        //
        //   5 packet(s) transmitted, 5 packet(s) received, 0.00% packet loss
        //
        // A real NE40E puts each on its own:
        //
        //     4 packet(s) transmitted
        //     4 packet(s) received
        //     0.00% packet loss
        //
        // Reading only the first meant a ping that answered every probe was
        // reported as total loss, beside a minimum, average and maximum that
        // had been read correctly.
        //
        // Nothing defaults here. A count nobody printed stays None, and a
        // result with no statistics at all is an error rather than a claim
        // that the address did not answer (rule 3, ADR-0006): ping is the
        // query most likely to be read as a yes or no, so it is the worst
        // place to guess.
        let mut sent: Option<u32> = None;
        let mut recv: Option<u32> = None;
        let mut loss: Option<f64> = None;
        let mut min = None;
        let mut avg = None;
        let mut max = None;

        for line in raw.lines() {
            let line = line.trim();

            if line.contains("transmitted")
                || line.contains("received")
                || line.contains("packet loss")
            {
                // One line or three: splitting on commas covers both, since a
                // line with no comma is a single field.
                for field in line.split(',') {
                    let field = field.trim();
                    let Some(first) = field.split_whitespace().next() else {
                        continue;
                    };
                    if field.contains("transmitted") {
                        sent = first.parse().ok();
                    } else if field.contains("received") {
                        recv = first.parse().ok();
                    } else if field.contains("packet loss") {
                        loss = first.trim_end_matches('%').parse().ok();
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

        // No statistics at all: the router said something this driver does not
        // understand, and saying so is the only honest answer. Reporting a
        // reachability failure that did not happen sends someone looking for a
        // problem that does not exist.
        let (Some(sent), Some(recv)) = (sent, recv) else {
            return Err(DriverError::ParseError(
                "no ping statistics found in the router's output".to_string(),
            ));
        };

        Ok(PingResult {
            packets_sent: sent,
            packets_received: recv,
            // Derived only when the router did not print it, and from counts
            // it did print.
            packet_loss_percent: loss.unwrap_or_else(|| {
                if sent == 0 {
                    100.0
                } else {
                    (sent - recv) as f64 * 100.0 / sent as f64
                }
            }),
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

    /// Reads a BGP route: best-path flags, next hop, MED, local preference
    /// and AS path.
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // VRP answers in two shapes, and which one arrives depends on the
        // command. `display bgp routing-table <network> <mask>` — what the
        // catalogue sends for a route query — prints a block per path:
        //
        //  BGP routing table entry information of 203.0.113.0/24:
        //  From: 192.0.2.10 (192.0.2.10)
        //  Original nexthop: 192.0.2.10
        //  AS-path 64498, origin igp, MED 0, pref-val 0, valid, external, best
        //
        // `display bgp routing-table regular-expression ...`, which the AS
        // query sends, prints the columns handled further down. Reading only
        // the columns meant a real NE40E answered every route query with no
        // paths at all.
        if raw.contains("BGP routing table entry information of") {
            return Ok(parse_detail_blocks(raw));
        }

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
                || header.starts_with("Origin")
                || header.starts_with("RPKI")
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
                rpki: fields
                    .rpki
                    .map(RpkiValidation::from_router)
                    .unwrap_or_default(),
                ..BgpPath::default()
            });
        }

        Ok(BgpRouteResult::new(paths, raw).partial(unreadable))
    }

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        // Shared reader: the tables differ in headers, not in what a row means.
        Ok(crate::summary::parse(raw))
    }
}

/// Reads VRP's per-path detail blocks.
fn parse_detail_blocks(raw: &str) -> BgpRouteResult {
    let mut paths: Vec<BgpPath> = Vec::new();
    let mut unreadable = 0usize;
    let mut current: Option<BgpPath> = None;
    let mut partial_path = false;
    let mut in_community = false;

    for line in raw.lines() {
        let line = line.trim();

        if let Some(rest) = line.strip_prefix("BGP routing table entry information of") {
            in_community = false;
            if let Some(path) = current.take() {
                paths.push(path);
            }
            current = Some(BgpPath {
                prefix: parse_network(rest.trim().trim_end_matches(':')),
                ..BgpPath::default()
            });
            continue;
        }

        let Some(path) = current.as_mut() else {
            continue;
        };

        if let Some(rest) = line.strip_prefix("From:") {
            // `From: 192.0.2.10 (192.0.2.10)` — the session, then the peer's
            // router id. The address is the one that matters here.
            path.peer = rest.split_whitespace().next().and_then(parse_hop);
        } else if let Some(rest) = line.strip_prefix("Original nexthop:") {
            path.next_hop = rest.split_whitespace().next().and_then(parse_hop);
        } else if let Some(rest) = line.strip_prefix("Community:") {
            in_community = true;
            append_communities(rest, path);
        } else if let Some(rest) = line.strip_prefix("Large-Community:") {
            in_community = false;
            append_communities(rest, path);
        } else if in_community && (line.starts_with('<') || line.starts_with("...") || (line.contains(':') && !line.contains(' '))) {
            append_communities(line, path);
        } else if let Some(rest) = line.strip_prefix("AS-path") {
            in_community = false;
            match read_attributes(rest, path) {
                Ok(()) => {}
                Err(()) => partial_path = true,
            }
        } else if line.starts_with("Qos information")
            || line.starts_with("Route Duration")
            || line.starts_with("Direct Out-interface")
            || line.starts_with("Relay")
            || line.starts_with("Aggregator")
            || line.starts_with("Not advertised")
            || line.starts_with("Advertised to")
            || line.starts_with("Ext-Community:")
            || line.starts_with("BGP local router ID")
            || line.starts_with("Local AS number")
            || line.starts_with("Paths:")
            || line.is_empty()
        {
            in_community = false;
            // Known and carrying nothing the model holds.
        } else {
            in_community = false;
            unreadable += 1;
        }
    }

    if let Some(path) = current.take() {
        paths.push(path);
    }

    let result = BgpRouteResult::new(paths, raw);
    // A set in the path is not something the model can hold yet, and the
    // sequence stops at it; say so rather than present a shortened path as
    // whole.
    let result = if partial_path {
        result.partial(1)
    } else {
        result
    };
    result.partial(unreadable)
}

fn append_communities(text: &str, path: &mut BgpPath) {
    path.communities.extend(
        text.split(',')
            .map(|c| c.trim().trim_matches(|c| c == '<' || c == '>').trim())
            .filter(|c| !c.is_empty() && *c != "...")
            .map(Community::parse),
    );
}

/// Reads the attribute line that follows `AS-path`.
///
///   64498 {64496 64497}, origin igp, MED 0, pref-val 0, valid, external, best
///
/// Returns `Err` when the path contained an AS_SET, whose members are a set
/// rather than a sequence and are therefore not appended to it.
fn read_attributes(rest: &str, path: &mut BgpPath) -> Result<(), ()> {
    let mut had_set = false;
    for (index, attribute) in rest.split(',').enumerate() {
        let attribute = attribute.trim();
        if index == 0 {
            // `AS-path Nil` is VRP for a path this router originated.
            if attribute.eq_ignore_ascii_case("Nil") {
                continue;
            }
            let (sequence, set) = read_as_sequence(attribute);
            path.as_path = sequence;
            had_set = set;
            continue;
        }

        let mut words = attribute.split_whitespace();
        match (words.next(), words.next()) {
            (Some("origin"), Some(origin)) => {
                path.origin = match origin.to_ascii_lowercase().as_str() {
                    "igp" => Some(Origin::Igp),
                    "egp" => Some(Origin::Egp),
                    "incomplete" => Some(Origin::Incomplete),
                    _ => None,
                }
            }
            // Absent is not zero: a path with no MED simply has no `MED`
            // attribute here, and it stays None (ADR-0006).
            (Some("MED"), Some(value)) => path.med = value.parse().ok(),
            (Some("localpref"), Some(value)) => path.local_pref = value.parse().ok(),
            (Some("pref-val"), Some(value)) => path.weight = value.parse().ok(),
            (Some("valid"), _) => path.is_valid = Some(true),
            (Some("best"), _) => path.is_best = true,
            _ => {}
        }
    }

    if had_set {
        Err(())
    } else {
        Ok(())
    }
}

/// Reads an AS sequence, stopping at an AS_SET.
fn read_as_sequence(text: &str) -> (Vec<u32>, bool) {
    let mut sequence = Vec::new();
    for token in text.split_whitespace() {
        if token.starts_with('{') {
            return (sequence, true);
        }
        if let Some(asn) = parse_asn(token) {
            sequence.push(asn);
        }
    }
    (sequence, false)
}

/// Reads an AS number in either notation VRP prints.
///
/// A 32-bit ASN arrives as `1.0` rather than `65536` — asdot, which is what
/// VRP uses by default. Read as a plain integer it fails to parse, and a
/// parser that drops what it cannot read turns a ten-hop path into a four-hop
/// one that looks perfectly plausible.
fn parse_asn(token: &str) -> Option<u32> {
    if let Some((high, low)) = token.split_once('.') {
        let high: u32 = high.parse().ok()?;
        let low: u32 = low.parse().ok()?;
        if low > u16::MAX as u32 || high > u16::MAX as u32 {
            return None;
        }
        return Some(high * 65536 + low);
    }
    token.parse().ok()
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

    #[test]
    fn parses_tabular_with_rpki_column() {
        let raw = "\
 BGP Local router ID is 192.0.2.1
 Status codes: * - valid, > - best, d - damped, x - best external, a - add path,
               Origin : i - IGP, e - EGP, ? - incomplete
 RPKI validation codes: V - valid, I - invalid, N - not-found

 Total Number of Routes: 4
    Network            NextHop         MED   LocPrf   PrefVal Path/Ogn
*>  V 198.51.100.0/24   192.0.2.10             100        0    64512 64513i
*   N 198.51.101.0/24   192.0.2.11             100        0    64512 64514i
*>  I 198.51.102.0/24   192.0.2.12             100        0    64512 64515?
*   N                   192.0.2.13             100        0    64512 64516i
";
        let result = HuaweiVrpDriver.parse_bgp_route(raw).unwrap();
        assert_eq!(result.completeness, Completeness::Complete);
        assert_eq!(result.paths.len(), 4);
        assert_eq!(result.paths[0].rpki.status, RpkiStatus::Valid);
        assert_eq!(result.paths[0].rpki.source, crate::driver::RpkiSource::Router);
        assert_eq!(result.paths[1].rpki.status, RpkiStatus::NotFound);
        assert_eq!(result.paths[2].rpki.status, RpkiStatus::Invalid);
        assert_eq!(result.paths[3].rpki.status, RpkiStatus::NotFound);
        assert_eq!(result.paths[3].prefix.unwrap().to_string(), "198.51.102.0/24");
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

#[cfg(test)]
mod real_ne40e_tests {
    use super::*;
    use crate::driver::Completeness;

    /// Captured from a Huawei NE40E running VRP 8.180 (V800R011C00SPC607) in
    /// `lab/`, peering with three FRR speakers. This is the answer to the
    /// command the catalogue sends, which is not the format the column reader
    /// was written for.
    const REAL_DETAIL: &str = r#" BGP local router ID : 192.0.2.1
 Local AS number : 64499
 Paths:   3 available, 1 best, 1 select, 0 best-external, 0 add-path
 BGP routing table entry information of 203.0.113.0/24:
 From: 192.0.2.10 (192.0.2.10)  
 Route Duration: 0d00h15m42s
 Direct Out-interface: Ethernet1/0/2
 Original nexthop: 192.0.2.10
 Qos information : 0x0
 AS-path 64498, origin igp, MED 0, pref-val 0, valid, external, best, select, pre 255
 Not advertised to any peer yet

 BGP routing table entry information of 203.0.113.0/24:
 From: 192.0.2.6 (192.0.2.6)  
 Original nexthop: 192.0.2.6
 AS-path 64497 64498, origin igp, MED 100, pref-val 0, valid, external, pre 255, not preferred for AS-Path
 Not advertised to any peer yet

 BGP routing table entry information of 203.0.113.0/24:
 From: 192.0.2.2 (192.0.2.2)  
 Original nexthop: 192.0.2.2
 AS-path 64496 64496 64496 1.0 1.1 1.2 1.3 1.4 1.5 64498, origin igp, pref-val 0, valid, external, pre 255, not preferred for AS-Path
 Not advertised to any peer yet
"#;

    /// The route query returned nothing at all before this: the driver read
    /// the column format, and `display bgp routing-table <network> <mask>`
    /// answers in blocks.
    #[test]
    fn the_detail_format_is_read() {
        let result = HuaweiVrpDriver
            .parse_bgp_route(REAL_DETAIL)
            .expect("real NE40E output");

        assert_eq!(result.paths.len(), 3, "three paths, none of them lost");
        for path in &result.paths {
            assert_eq!(
                path.prefix.map(|p| p.to_string()).as_deref(),
                Some("203.0.113.0/24")
            );
        }
    }

    /// VRP prints 32-bit ASNs in asdot: `1.0` is 65536. Parsed as a plain
    /// integer it fails, and a parser that drops what it cannot read turns a
    /// ten-hop path into a four-hop one that looks perfectly plausible.
    #[test]
    fn asdot_as_numbers_are_read_as_numbers() {
        let result = HuaweiVrpDriver
            .parse_bgp_route(REAL_DETAIL)
            .expect("real NE40E output");

        assert_eq!(
            result.paths[2].as_path,
            vec![64496, 64496, 64496, 65536, 65537, 65538, 65539, 65540, 65541, 64498]
        );
    }

    /// Absent, zero and set, in one answer (ADR-0006).
    #[test]
    fn an_absent_med_is_not_a_med_of_zero() {
        let result = HuaweiVrpDriver
            .parse_bgp_route(REAL_DETAIL)
            .expect("real NE40E output");

        let meds: Vec<Option<u32>> = result.paths.iter().map(|p| p.med).collect();
        assert_eq!(meds, vec![Some(0), Some(100), None]);
    }

    #[test]
    fn the_best_path_is_the_one_marked_best() {
        let result = HuaweiVrpDriver
            .parse_bgp_route(REAL_DETAIL)
            .expect("real NE40E output");

        let best: Vec<bool> = result.paths.iter().map(|p| p.is_best).collect();
        assert_eq!(best, vec![true, false, false]);
        assert_eq!(
            result.best().and_then(|p| p.peer).map(|p| p.to_string()),
            Some("192.0.2.10".to_string()),
            "the path's peer is the session it arrived on"
        );
    }

    /// VRP closes the brace and separates the members with spaces, unlike
    /// RouterOS. The members are still a set, not a sequence.
    #[test]
    fn an_as_set_stops_the_path_and_marks_the_result_partial() {
        let raw = r#" BGP routing table entry information of 198.51.100.0/24:
 From: 192.0.2.10 (192.0.2.10)  
 Original nexthop: 192.0.2.10
 AS-path 64498 {64496 64497}, origin igp, MED 0, pref-val 0, valid, external, best, select, pre 255
 Aggregator: AS 64498, Aggregator ID 192.0.2.10
"#;

        let result = HuaweiVrpDriver
            .parse_bgp_route(raw)
            .expect("real NE40E output");

        assert_eq!(result.paths[0].as_path, vec![64498]);
        assert!(matches!(result.completeness, Completeness::Partial { .. }));
    }

    /// The column format still arrives, from the AS-path query, and still has
    /// to be read.
    #[test]
    fn the_column_format_still_works() {
        let raw = "\
 BGP Local router ID is 192.0.2.1
 Status codes: * - valid, > - best, d - damped, h - history, i - internal
   Network            NextHop         MED   LocPrf  PrefVal Path/Ogn
*>  198.51.100.0/24    192.0.2.254      10      150        0 65100 65500i
";
        let result = HuaweiVrpDriver.parse_bgp_route(raw).expect("column output");
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].med, Some(10));
    }

    /// Captured from the NE40E in `lab/`. The statistics arrive on three
    /// lines; the documented shape puts them on one, and reading only that
    /// reported a ping that answered every probe as total loss.
    #[test]
    fn statistics_on_separate_lines_are_read() {
        let raw = r#"  PING 203.0.113.10: 56  data bytes, press CTRL_C to break
    Reply from 203.0.113.10: bytes=56 Sequence=1 ttl=63 time=2 ms
    Reply from 203.0.113.10: bytes=56 Sequence=2 ttl=63 time=1 ms
    Reply from 203.0.113.10: bytes=56 Sequence=3 ttl=63 time=1 ms
    Reply from 203.0.113.10: bytes=56 Sequence=4 ttl=63 time=1 ms

  --- 203.0.113.10 ping statistics ---
    4 packet(s) transmitted
    4 packet(s) received
    0.00% packet loss
    round-trip min/avg/max = 1/1/2 ms
"#;

        let result = HuaweiVrpDriver.parse_ping(raw).expect("real NE40E output");

        assert_eq!(result.packets_sent, 4);
        assert_eq!(result.packets_received, 4);
        assert_eq!(result.packet_loss_percent, 0.0);
        assert_eq!(result.min_rtt_ms, Some(1.0));
        assert_eq!(result.max_rtt_ms, Some(2.0));
    }

    /// The documented shape still arrives on other builds.
    #[test]
    fn statistics_on_one_line_are_read() {
        let raw = "\
  --- 198.51.100.1 ping statistics ---
  5 packet(s) transmitted, 4 packet(s) received, 20.00% packet loss
  round-trip min/avg/max = 1/2/4 ms
";
        let result = HuaweiVrpDriver.parse_ping(raw).expect("documented output");

        assert_eq!(result.packets_sent, 5);
        assert_eq!(result.packets_received, 4);
        assert_eq!(result.packet_loss_percent, 20.0);
    }

    /// The old parser defaulted loss to 100, so output it did not understand
    /// became a reachability failure that had not happened. Ping is the query
    /// most likely to be read as a yes or no; refusing is the honest answer.
    #[test]
    fn output_without_statistics_is_refused_rather_than_called_loss() {
        let raw = "  PING 203.0.113.10: 56  data bytes, press CTRL_C to break\n";

        let error = HuaweiVrpDriver
            .parse_ping(raw)
            .expect_err("no statistics means no answer");

        assert!(
            matches!(error, DriverError::ParseError(_)),
            "got {error:?}, which is not a refusal"
        );
    }
}
