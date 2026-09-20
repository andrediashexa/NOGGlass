//! Reading traceroute output.
//!
//! Vendors print traceroute in shapes that differ in decoration but agree on
//! the substance: a hop number, an address or a name, and one or more round
//! trip times. Rather than a parser per vendor, this is one reader with the
//! vendor differences expressed as options — there is far less genuine variety
//! here than in BGP tables.
//!
//! What matters is what the naive version gets wrong:
//!
//! * **A silent hop is a hop.** `3  * * *` means the router at hop 3 did not
//!   answer, which is different from hop 3 not existing. Dropping it renumbers
//!   everything after it and hides where the path stopped.
//! * **Probes are kept, not averaged.** Three probes of 1 ms, 1 ms and 400 ms
//!   say something an average of 134 ms does not.
//! * **A hop can answer from several addresses**, when load balancing sends
//!   probes down different paths. The first is recorded and the rest are not
//!   invented away.

use crate::driver::{TracerouteHop, TracerouteResult};
use regex::Regex;
use std::sync::LazyLock;

/// A hop line: leading number, then whatever the vendor prints.
static HOP_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(\d{1,3})\s+(.*)$").expect("hop line regex"));

/// `name (address)` or a bare address.
static NAME_AND_ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<name>[A-Za-z0-9][\w\.\-]*)\s+\((?P<address>[0-9a-fA-F:.]+)\)")
        .expect("name and address regex")
});

/// A round trip time with its unit, in any of the forms vendors print:
/// `1.234 ms`, `1.234ms`, `1ms234us`, `1234 us`.
/// No word boundary after the unit: RouterOS writes `1ms234us` with no
/// separator, and `\b` between `s` and `2` does not match, which dropped the
/// millisecond half of every RouterOS timing.
static RTT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<value>\d+(?:\.\d+)?)\s*(?P<unit>ms|us|s)").expect("rtt regex")
});

/// Reads one traceroute output.
///
/// `target` is what was asked about, kept so the result can say what it traced.
pub fn parse(target: &str, raw: &str) -> TracerouteResult {
    let mut hops: Vec<TracerouteHop> = Vec::new();

    for line in raw.lines() {
        let Some(captures) = HOP_LINE.captures(line) else {
            continue;
        };
        let Ok(number) = captures[1].parse::<u32>() else {
            continue;
        };
        let rest = captures[2].trim();

        // Skip a header that happens to start with a number, and the summary
        // lines some vendors print after the hops.
        if rest.is_empty() || rest.starts_with("packets") {
            continue;
        }

        let (hostname, ip) = read_endpoint(rest);
        let rtt_ms = read_times(rest);

        // A line with neither an address nor a timing is not a hop, however
        // much it looks like one.
        if ip.is_none() && hostname.is_none() && rtt_ms.is_empty() && !is_silent_hop(rest) {
            continue;
        }

        hops.push(TracerouteHop {
            hop: number,
            ip,
            hostname,
            rtt_ms,
        });
    }

    TracerouteResult {
        target: target.to_string(),
        hops,
        raw_output: raw.to_string(),
    }
}

/// A hop that answered nothing: the line carries only markers and counters.
///
/// `*` is the common marker, but RouterOS writes the word `timeout` in a table
/// that also carries a loss percentage and a probe count. Those columns are not
/// an answer, and a line that has nothing else is a hop that stayed silent —
/// which MUST keep its number. Dropping it renumbers every hop after it and
/// moves where the path appears to stop.
fn is_silent_hop(rest: &str) -> bool {
    let meaningful: Vec<&str> = rest
        .split_whitespace()
        .filter(|token| {
            let token = *token;
            // Markers.
            if matches!(token, "*" | "!" | "???" | "?") || token.eq_ignore_ascii_case("timeout") {
                return false;
            }
            // A percentage column, such as the `100%` RouterOS prints for loss.
            if let Some(number) = token.strip_suffix('%') {
                if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return false;
                }
            }
            // A bare counter, such as the number of probes sent.
            !token.chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    meaningful.is_empty()
}

/// Reads the name and address a hop answered with.
fn read_endpoint(rest: &str) -> (Option<String>, Option<String>) {
    if let Some(captures) = NAME_AND_ADDRESS.captures(rest) {
        return (
            Some(captures["name"].to_string()),
            Some(captures["address"].to_string()),
        );
    }

    // A bare address, which is what every vendor prints with numeric output.
    for token in rest.split_whitespace() {
        let candidate = token.trim_matches(|c| c == '(' || c == ')' || c == ',');
        if candidate.parse::<std::net::IpAddr>().is_ok() {
            return (None, Some(candidate.to_string()));
        }
    }

    // A name with no address: the hop answered and resolution succeeded, but
    // the vendor printed only the name.
    for token in rest.split_whitespace() {
        if token.contains('.')
            && token
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && RTT.find(token).is_none()
        {
            return (Some(token.to_string()), None);
        }
    }

    (None, None)
}

/// Reads every round trip time on the line, in milliseconds.
///
/// RouterOS prints `1ms234us`, which is one time in two parts rather than two
/// times; that is handled by combining a millisecond value immediately followed
/// by a microsecond one.
fn read_times(rest: &str) -> Vec<f64> {
    let mut times = Vec::new();
    let mut pending_ms: Option<f64> = None;

    for capture in RTT.captures_iter(rest) {
        let Ok(value) = capture["value"].parse::<f64>() else {
            continue;
        };
        match &capture["unit"] {
            "ms" => {
                if let Some(previous) = pending_ms.take() {
                    times.push(previous);
                }
                pending_ms = Some(value);
            }
            "us" => match pending_ms.take() {
                // `1ms234us` is 1.234 ms, not two separate probes.
                Some(milliseconds) => times.push(milliseconds + value / 1000.0),
                None => times.push(value / 1000.0),
            },
            "s" => {
                if let Some(previous) = pending_ms.take() {
                    times.push(previous);
                }
                times.push(value * 1000.0);
            }
            _ => {}
        }
    }

    if let Some(remaining) = pending_ms {
        times.push(remaining);
    }
    times
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cisco and Juniper print name, address and three probes.
    #[test]
    fn reads_hops_with_names_and_three_probes() {
        let raw = "\
traceroute to 198.51.100.10 (198.51.100.10), 30 hops max, 60 byte packets
 1  edge-01.example.net (192.0.2.254)  0.412 ms  0.398 ms  0.431 ms
 2  transit-a.example.net (192.0.2.1)  1.204 ms  1.180 ms  1.233 ms
 3  198.51.100.10 (198.51.100.10)  2.551 ms  2.498 ms  2.602 ms
";
        let result = parse("198.51.100.10", raw);
        assert_eq!(result.hops.len(), 3);

        let first = &result.hops[0];
        assert_eq!(first.hop, 1);
        assert_eq!(first.hostname.as_deref(), Some("edge-01.example.net"));
        assert_eq!(first.ip.as_deref(), Some("192.0.2.254"));
        assert_eq!(first.rtt_ms, vec![0.412, 0.398, 0.431]);
        assert_eq!(
            first.rtt_ms.len(),
            3,
            "the probes are kept, not averaged into one"
        );
    }

    /// The case that matters: a hop that did not answer still occupies its
    /// number, or every hop after it is renumbered and the path is wrong.
    #[test]
    fn a_silent_hop_is_kept_with_no_address_and_no_timings() {
        let raw = "\
 1  192.0.2.254  0.412 ms  0.398 ms  0.431 ms
 2  * * *
 3  198.51.100.10  2.551 ms  2.498 ms  2.602 ms
";
        let result = parse("198.51.100.10", raw);
        assert_eq!(result.hops.len(), 3, "the silent hop is still a hop");

        let silent = &result.hops[1];
        assert_eq!(silent.hop, 2);
        assert_eq!(silent.ip, None);
        assert_eq!(silent.hostname, None);
        assert!(silent.rtt_ms.is_empty());

        assert_eq!(result.hops[2].hop, 3, "numbering is not shifted");
    }

    #[test]
    fn reads_bare_addresses_from_numeric_output() {
        let raw = " 1  192.0.2.254  0.412 ms\n 2  2001:db8::1  1.204 ms\n";
        let result = parse("2001:db8::1", raw);
        assert_eq!(result.hops[0].ip.as_deref(), Some("192.0.2.254"));
        assert_eq!(result.hops[0].hostname, None);
        assert_eq!(result.hops[1].ip.as_deref(), Some("2001:db8::1"));
    }

    /// RouterOS prints one time as `1ms234us`, which is 1.234 ms and not two
    /// probes of 1 ms and 234 microseconds.
    #[test]
    fn reads_routeros_split_units_as_one_time() {
        let raw = "    1 192.0.2.254                     1ms234us\n";
        let result = parse("198.51.100.10", raw);
        assert_eq!(result.hops[0].rtt_ms, vec![1.234]);
    }

    #[test]
    fn reads_microseconds_and_seconds() {
        assert_eq!(read_times("500us"), vec![0.5]);
        assert_eq!(read_times("1.5 s"), vec![1500.0]);
        assert_eq!(read_times("1.234 ms 2.5 ms"), vec![1.234, 2.5]);
    }

    /// Huawei prints the hop number, the address and the times without
    /// parentheses.
    #[test]
    fn reads_huawei_output() {
        let raw = "\
 traceroute to 198.51.100.10(198.51.100.10), max hops: 30 ,packet length: 40
 1 192.0.2.254 1 ms 1 ms 2 ms
 2 * * *
 3 198.51.100.10 3 ms 2 ms 3 ms
";
        let result = parse("198.51.100.10", raw);
        assert_eq!(result.hops.len(), 3);
        assert_eq!(result.hops[0].ip.as_deref(), Some("192.0.2.254"));
        assert_eq!(result.hops[0].rtt_ms, vec![1.0, 1.0, 2.0]);
        assert!(result.hops[1].rtt_ms.is_empty());
    }

    #[test]
    fn the_header_is_not_a_hop() {
        let raw =
            "traceroute to 198.51.100.10 (198.51.100.10), 30 hops max\n 1  192.0.2.254  1 ms\n";
        let result = parse("198.51.100.10", raw);
        assert_eq!(result.hops.len(), 1);
        assert_eq!(result.hops[0].hop, 1);
    }

    #[test]
    fn the_raw_output_is_preserved() {
        let raw = " 1  192.0.2.254  1 ms\n";
        assert_eq!(parse("198.51.100.10", raw).raw_output, raw);
    }

    #[test]
    fn output_with_no_hops_parses_to_no_hops() {
        let result = parse("198.51.100.10", "traceroute: unknown host\n");
        assert!(result.hops.is_empty());
        assert_eq!(result.target, "198.51.100.10");
    }
}
