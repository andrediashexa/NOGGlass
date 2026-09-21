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

/// Where a table's columns sit, taken from its header.
///
/// ```text
///      Network          Next Hop            Metric LocPrf Weight Path
///  *>   198.51.100.0/25  192.0.2.2                0             0 64496 65536 i
/// ```
///
/// The numbers are right-aligned under their headings and any of them can be
/// blank. Reading them as "the leading numbers of what is left" works until one
/// is missing, and then the first AS of the path becomes the weight — which is
/// what a real IOS-XE table did.
#[derive(Debug, Clone, Copy)]
struct Columns {
    metric_end: usize,
    local_pref_end: usize,
    weight_end: usize,
    path_start: usize,
}

impl Columns {
    fn from_header(header: &str) -> Option<Self> {
        let end_of = |name: &str| header.find(name).map(|start| start + name.len());
        Some(Self {
            metric_end: end_of("Metric")?,
            local_pref_end: end_of("LocPrf")?,
            weight_end: end_of("Weight")?,
            path_start: header.find("Path")?,
        })
    }

    /// Reads the three numeric columns and the text of the AS path.
    ///
    /// The columns are right-aligned, so each number is matched to the column
    /// whose heading *ends* where the number does. That survives a value wider
    /// than its heading, which overflows to the left, and it survives a blank
    /// column, which is what broke the previous reader: with LocPrf empty
    /// there were two numbers where it expected three, and the first AS of the
    /// path became the weight.
    fn read(&self, row: &str) -> (Option<u32>, Option<u32>, Option<u32>, Option<String>) {
        let path = row
            .get(self.path_start.min(row.len())..)
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());

        let mut metric = None;
        let mut local_pref = None;
        let mut weight = None;

        // Every number that sits before the AS path, with where it ends.
        for (offset, token) in token_offsets(row) {
            let ends_at = offset + token.len();
            if ends_at > self.path_start {
                break;
            }
            let Ok(value) = token.parse::<u32>() else {
                continue;
            };

            // Three characters of slack: vendors pad these columns by a space
            // or two and the headings are not always the same width. More
            // slack than that and a number could be claimed by its neighbour,
            // since the headings themselves are only seven apart.
            let nearest = [
                (self.metric_end, &mut metric),
                (self.local_pref_end, &mut local_pref),
                (self.weight_end, &mut weight),
            ]
            .into_iter()
            .filter(|(column_end, _)| ends_at.abs_diff(*column_end) <= 3)
            .min_by_key(|(column_end, _)| ends_at.abs_diff(*column_end));

            match nearest {
                Some((_, slot)) => *slot = Some(value),
                // A number before the path that lines up with no column at
                // all means this row is not laid out the way its header says.
                // Reading the rest by offset would put the weight in the AS
                // path, so the caller falls back to counting tokens.
                None => return (None, None, None, None),
            }
        }

        // A row can also be misaligned the other way: the weight sits past
        // where the header says the path begins, so the loop above stopped
        // before it and the path now starts with a number that is not an AS.
        // Reading that as the first hop of the path is exactly the kind of
        // plausible wrong answer this reader exists to avoid.
        if weight.is_none() {
            if let Some((offset, token)) =
                token_offsets(row).find(|(offset, _)| *offset >= self.path_start)
            {
                let ends_at = offset + token.len();
                if token.parse::<u32>().is_ok() && ends_at.abs_diff(self.weight_end) <= 3 {
                    return (None, None, None, None);
                }
            }
        }

        (metric, local_pref, weight, path)
    }
}

/// Every whitespace-separated token of a line, with its byte offset.
fn token_offsets(line: &str) -> impl Iterator<Item = (usize, &str)> {
    line.split_whitespace()
        .map(move |token| (token.as_ptr() as usize - line.as_ptr() as usize, token))
}

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
    let mut columns: Option<Columns> = None;

    for line in raw.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let body = trimmed.trim_start();
        if body.starts_with("Network") {
            // The header is the only thing that says where the columns are,
            // and without it the numeric ones cannot be told apart: a row
            // whose LocPrf is blank has two numbers where the reader expects
            // three, and the first AS of the path was read as the weight.
            columns = Columns::from_header(trimmed);
            continue;
        }
        if body.starts_with("BGP table")
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

        // The legend wraps, and its continuation lines begin with a status
        // letter: `r RIB-failure, S Stale, m multipath, ...` passes the test
        // above and was read as a route with no prefix — an empty row in the
        // interface, from a line explaining what the letters mean.
        if body.contains("RIB-failure") || body.contains("best-external") {
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

        // With the header, each numeric column is read from its own slice of
        // the line, so a blank one stays absent instead of shifting the rest.
        // Without it, fall back to the old guess: at most three leading
        // numbers are metrics and the rest is the path.
        let (metric, local_pref, weight, as_path_text) = match columns.as_ref() {
            Some(columns) => columns.read(trimmed),
            None => (None, None, None, None),
        };

        let (numbers, as_path): (Vec<Option<u32>>, Vec<u32>) = match as_path_text {
            Some(text) => (
                vec![metric, local_pref, weight],
                text.split_whitespace()
                    .filter_map(|token| token.parse::<u32>().ok())
                    .collect(),
            ),
            None => {
                let metric_columns = remaining
                    .iter()
                    .take(3)
                    .take_while(|t| t.bytes().all(|b| b.is_ascii_digit()))
                    .count();
                (
                    remaining
                        .iter()
                        .take(metric_columns)
                        .map(|t| t.parse().ok())
                        .collect(),
                    remaining
                        .iter()
                        .skip(metric_columns)
                        .filter_map(|t| t.parse::<u32>().ok())
                        .collect(),
                )
            }
        };

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
            med: numbers.first().copied().flatten(),
            local_pref: numbers.get(1).copied().flatten(),
            weight: numbers.get(2).copied().flatten(),
            origin,
            ..BgpPath::default()
        });
    }

    BgpRouteResult::new(paths, raw).partial(unreadable)
}
