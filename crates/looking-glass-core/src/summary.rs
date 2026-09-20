//! Reading BGP session summaries.
//!
//! "Which of my sessions is down" is most of why an operator opens a looking
//! glass, and it is the one question raw text answers badly: the state column
//! is in a different place on every platform, and what it prints when a session
//! is *up* is a number, not a word.
//!
//! That last point is the trap. Cisco, Huawei and Datacom print the number of
//! prefixes received where the state would go, and only print a word —
//! `Idle`, `Active`, `Connect` — when the session is **not** established. A
//! parser that looks for "Established" therefore finds it almost never, and one
//! that treats any word as a state reports healthy sessions as broken.
//!
//! So the rule here is the opposite of the obvious one: **a number in the state
//! column means the session is up**.

use crate::driver::{BgpPeerSummary, BgpSummaryResult};
use regex::Regex;
use std::sync::LazyLock;

/// The words vendors print for a session that is not established.
const DOWN_STATES: &[&str] = &[
    "idle",
    "active",
    "connect",
    "opensent",
    "openconfirm",
    "down",
    "shutdown",
    "no-neg",
    "closing",
];

/// `BGP router identifier 192.0.2.1, local AS number 65001`
static ROUTER_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:BGP\s+)?(?:local\s+)?router\s+ID\s+(?:is\s+)?(?P<id>[0-9a-fA-F:.]+)|identifier\s+(?P<id2>[0-9a-fA-F:.]+)",
    )
    .expect("router line regex")
});

/// `local AS number 65001`, `Local AS Number : 65001`
static LOCAL_AS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)local\s+AS(?:\s+number)?\s*:?\s*(?P<asn>\d+)").expect("local AS regex")
});

/// Reads a summary table.
///
/// The peer address is what anchors a row: a line that starts with an address
/// is a session, and anything else is a header, a banner or a total.
pub fn parse(raw: &str) -> BgpSummaryResult {
    let mut peers = Vec::new();
    let mut router_id = None;
    let mut local_as = None;

    for line in raw.lines() {
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

        if let Some(peer) = parse_peer_row(line) {
            peers.push(peer);
        }
    }

    BgpSummaryResult {
        router_id,
        local_as,
        peers,
        raw_output: raw.to_string(),
    }
}

/// Reads one session row, or `None` when the line is not one.
fn parse_peer_row(line: &str) -> Option<BgpPeerSummary> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 3 {
        return None;
    }

    // A session row starts with the peer address.
    let peer_ip = fields[0];
    peer_ip.parse::<std::net::IpAddr>().ok()?;

    // The peer AS is the first number after the address that could be one.
    // Version columns (`4`) come first on Cisco and Huawei, so a bare 4 with
    // more numbers after it is skipped.
    let peer_as = fields[1..]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            let value: u32 = token.parse().ok()?;
            // The version column is 4 and is always followed by the AS.
            if value == 4 && index + 2 < fields.len() {
                return None;
            }
            Some(value)
        })
        .unwrap_or(0);

    // The state is the last field on every platform: either a word for a
    // session that is not up, or a prefix count for one that is.
    let last = fields.last().copied().unwrap_or_default();
    let last_lower = last.to_ascii_lowercase();

    let (state, prefixes_received) = if let Ok(count) = last.parse::<u32>() {
        // A number here means the session is established and this is how many
        // prefixes it sent. Vendors do not write "Established" in this column.
        ("Established".to_string(), count)
    } else if DOWN_STATES.iter().any(|down| last_lower.starts_with(down)) {
        (capitalise(last), 0)
    } else if last_lower.contains("established") {
        ("Established".to_string(), 0)
    } else {
        // Something we do not recognise. Report it verbatim rather than
        // guessing: an operator reading an unfamiliar word can look it up, and
        // a wrong guess about a session state is worse than an honest one.
        (last.to_string(), 0)
    };

    // Uptime is the field before the state, when it looks like a duration.
    let uptime = fields
        .get(fields.len().saturating_sub(2))
        .filter(|token| looks_like_uptime(token))
        .map(|token| (*token).to_string())
        .unwrap_or_default();

    Some(BgpPeerSummary {
        peer_ip: peer_ip.to_string(),
        peer_as,
        state,
        uptime,
        prefixes_received,
        // Only a few platforms report accepted separately; left absent rather
        // than copied from received, which would claim a filter passed
        // everything.
        prefixes_accepted: None,
    })
}

/// `01:23:45`, `2d04h`, `5w1d`, `never`.
fn looks_like_uptime(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    if lower == "never" {
        return true;
    }
    token.contains(':')
        || (token.chars().any(|c| c.is_ascii_digit())
            && token
                .chars()
                .any(|c| matches!(c.to_ascii_lowercase(), 'd' | 'h' | 'w' | 'm' | 'y')))
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cisco: the state column holds a prefix count when the session is up.
    #[test]
    fn a_number_in_the_state_column_means_the_session_is_up() {
        let raw = "\
BGP router identifier 192.0.2.1, local AS number 65001
BGP table version is 42, main routing table version 42

Neighbor        V           AS MsgRcvd MsgSent   TblVer  InQ OutQ Up/Down  State/PfxRcd
192.0.2.254     4        65100   12345   12300       42    0    0 05:12:33      850000
192.0.2.253     4        65200    4321    4300       42    0    0 02:01:10          12
192.0.2.252     4        65300       0       0        0    0    0 never    Idle
";
        let result = parse(raw);
        assert_eq!(result.router_id.as_deref(), Some("192.0.2.1"));
        assert_eq!(result.local_as, Some(65001));
        assert_eq!(result.peers.len(), 3);

        let up = &result.peers[0];
        assert_eq!(up.peer_ip, "192.0.2.254");
        assert_eq!(up.peer_as, 65100);
        assert_eq!(
            up.state, "Established",
            "a prefix count in the state column means the session is up"
        );
        assert_eq!(up.prefixes_received, 850_000);
        assert_eq!(up.uptime, "05:12:33");

        let down = &result.peers[2];
        assert_eq!(down.peer_as, 65300);
        assert_eq!(down.state, "Idle");
        assert_eq!(down.prefixes_received, 0);
        assert_eq!(down.uptime, "never");
    }

    /// The trap this module exists for: a parser looking for "Established"
    /// finds it on none of these rows, and would report every healthy session
    /// as broken.
    #[test]
    fn a_healthy_table_is_not_reported_as_broken() {
        let raw = "\
192.0.2.254     4        65100   12345   12300       42    0    0 05:12:33      850000
192.0.2.253     4        65200    4321    4300       42    0    0 02:01:10           7
";
        let result = parse(raw);
        assert!(
            result.peers.iter().all(|peer| peer.state == "Established"),
            "every session here is up: {:?}",
            result.peers.iter().map(|p| &p.state).collect::<Vec<_>>()
        );
    }

    /// Huawei prints the same shape with different headers.
    #[test]
    fn reads_huawei_tables() {
        let raw = "\
 BGP local router ID : 192.0.2.1
 Local AS number : 65001
 Total number of peers : 2

  Peer            V    AS  MsgRcvd  MsgSent  OutQ  Up/Down       State  PrefRcv
  192.0.2.254     4 65100    12345    12300     0  05:12:33   Established  850000
  192.0.2.252     4 65300        0        0     0  00:00:00        Active       0
";
        let result = parse(raw);
        assert_eq!(result.local_as, Some(65001));
        assert_eq!(result.peers.len(), 2);
        assert_eq!(result.peers[0].state, "Established");
        assert_eq!(result.peers[0].prefixes_received, 850_000);
        assert_eq!(result.peers[1].state, "Established");
    }

    #[test]
    fn a_session_in_idle_is_reported_as_idle() {
        let raw =
            "192.0.2.252     4        65300       0       0        0    0    0 never    Idle\n";
        let peer = &parse(raw).peers[0];
        assert_eq!(peer.state, "Idle");
        assert_eq!(
            peer.prefixes_received, 0,
            "a session that is not up sent no prefixes"
        );
    }

    #[test]
    fn ipv6_peers_read_the_same_way() {
        let raw = "2001:db8::1   4  65100  100  100  0  0  0 01:00:00  1200\n";
        let peer = &parse(raw).peers[0];
        assert_eq!(peer.peer_ip, "2001:db8::1");
        assert_eq!(peer.peer_as, 65100);
        assert_eq!(peer.prefixes_received, 1200);
    }

    #[test]
    fn headers_and_totals_are_not_sessions() {
        let raw = "\
Neighbor        V           AS MsgRcvd MsgSent   TblVer  InQ OutQ Up/Down  State/PfxRcd
 Total number of peers : 3        Peers in established state : 2
192.0.2.254     4        65100   12345   12300       42    0    0 05:12:33      850000
";
        assert_eq!(parse(raw).peers.len(), 1);
    }

    /// An unfamiliar state is reported verbatim: an operator can look a word
    /// up, but cannot recover from a wrong guess about whether a session is up.
    #[test]
    fn an_unrecognised_state_is_passed_through() {
        let raw = "192.0.2.252  4  65300  0  0  0  0  0  00:00:00  Clearing\n";
        assert_eq!(parse(raw).peers[0].state, "Clearing");
    }

    #[test]
    fn accepted_is_absent_rather_than_copied_from_received() {
        let raw = "192.0.2.254  4  65100  1  1  0  0  0  01:00:00  500\n";
        let peer = &parse(raw).peers[0];
        assert_eq!(peer.prefixes_received, 500);
        assert_eq!(
            peer.prefixes_accepted, None,
            "claiming accepted equals received would say a filter passed everything"
        );
    }

    #[test]
    fn the_raw_output_is_preserved() {
        let raw = "192.0.2.254  4  65100  1  1  0  0  0  01:00:00  500\n";
        assert_eq!(parse(raw).raw_output, raw);
    }

    #[test]
    fn a_table_with_no_sessions_parses_to_no_peers() {
        let result = parse("% BGP not active\n");
        assert!(result.peers.is_empty());
    }
}
