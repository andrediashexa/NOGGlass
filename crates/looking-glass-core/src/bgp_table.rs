//! Reading the tabular BGP output several vendors share.
//!
//! Cisco IOS-XE, IOS-XR and Datacom DmOS print the same table: status flags,
//! network, next hop, three right-aligned numeric columns, then the AS path and
//! an origin marker. The headers differ; the meaning of a row does not.
//!
//! The two things that make this harder than it looks are handled here once:
//! the status flags are glued to the network on IOS, and a continuation line
//! omits the network — while a next hop parses as a network too, as a host
//! route, so "does it parse" cannot tell the columns apart.

use crate::driver::{parse_hop, parse_network, BgpPath, BgpRouteResult, Origin};

/// Reads a tabular BGP table.
pub fn parse(raw: &str) -> BgpRouteResult {
    // Tabular `show bgp` output, the form IOS-XE and IOS-XR share:
    //
    //    Network          Next Hop   Metric LocPrf Weight Path
    // *>i198.51.100.0/24  192.0.2.254    10    150      0 65100 65500 i
    //
    // The status column is glued to the network on IOS, so it is split off
    // by character rather than by whitespace.
    let mut paths = Vec::new();
    let mut unreadable = 0usize;
    let mut current_prefix = None;

    for line in raw.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let body = trimmed.trim_start();
        if body.starts_with("Network")
            || body.starts_with("BGP table")
            || body.starts_with("Status codes")
            || body.starts_with("Origin codes")
            || body.starts_with("Route Distinguisher")
            || body.starts_with("Processed")
        {
            continue;
        }
        if !body.starts_with('*') && !body.starts_with("r>") && !body.starts_with('r') {
            unreadable += 1;
            continue;
        }

        // Status flags are the leading non-alphanumeric characters plus the
        // internal/external marker.
        let flags: String = body
            .chars()
            .take_while(|c| matches!(c, '*' | '>' | 'r' | 's' | 'd' | 'h' | 'i' | 'S' | ' '))
            .collect();
        let rest = &body[flags.len()..];
        let mut fields = rest.split_whitespace();

        let Some(first) = fields.next() else {
            unreadable += 1;
            continue;
        };

        // A continuation line omits the network and starts at the next
        // hop, and a next hop parses as a network too — as a host route —
        // so "does it parse" cannot decide. What decides: a network is
        // written in CIDR form, or it is followed by another address, which
        // is the next hop. Getting this wrong attributed a path to a /32 of
        // its own next hop.
        let second = fields.clone().next();
        let first_is_network =
            first.contains('/') || second.is_some_and(|token| parse_hop(token).is_some());

        let (prefix, next_hop_text) = if first_is_network {
            let parsed = parse_network(first);
            if parsed.is_some() {
                current_prefix = parsed;
            }
            (current_prefix, fields.next())
        } else {
            (current_prefix, Some(first))
        };

        let remaining: Vec<&str> = fields.collect();

        // Metric, LocPrf and Weight are right-aligned numbers, then the AS
        // path, then the origin marker. Every one of those is a number, so
        // the split is positional: at most three metric columns, and the
        // rest is the path. Taking numbers greedily swallowed the AS path
        // and left it empty.
        let metric_columns = remaining
            .iter()
            .take(3)
            .take_while(|t| t.bytes().all(|b| b.is_ascii_digit()))
            .count();
        let numbers: Vec<u32> = remaining
            .iter()
            .take(metric_columns)
            .filter_map(|t| t.parse().ok())
            .collect();
        let as_path: Vec<u32> = remaining
            .iter()
            .skip(metric_columns)
            .filter_map(|t| t.parse::<u32>().ok())
            .collect();
        let origin = remaining
            .last()
            .and_then(|t| t.chars().last())
            .and_then(Origin::from_marker);

        paths.push(BgpPath {
            prefix,
            next_hop: next_hop_text.and_then(parse_hop),
            is_best: flags.contains('>'),
            is_valid: Some(flags.contains('*')),
            as_path,
            // Positional: metric, then local preference, then weight.
            med: numbers.first().copied(),
            local_pref: numbers.get(1).copied(),
            weight: numbers.get(2).copied(),
            origin,
            ..BgpPath::default()
        });
    }

    BgpRouteResult::new(paths, raw).partial(unreadable)
}
