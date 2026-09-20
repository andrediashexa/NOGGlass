//! Reading ping output in the shape Unix-like platforms print.
//!
//! Nokia SR OS, Datacom DmOS and any Linux host running BIRD all print the
//! summary iputils established:
//!
//! ```text
//! 5 packets transmitted, 5 packets received, 0.00% packet loss
//! round-trip min/avg/max = 0.412/0.501/0.688 ms
//! ```
//!
//! Two things a naive reader gets wrong. A ping where **every** packet was lost
//! prints no round-trip line at all, and reporting 0 ms for it would claim a
//! reply that never came — so the timings stay absent. And the loss figure is
//! printed by the router; it is read rather than recomputed, because a router
//! that counts differently from us is telling us something.

use crate::driver::{DriverError, PingResult};
use regex::Regex;
use std::sync::LazyLock;

/// `5 packets transmitted, 5 packets received` and the SR OS spelling
/// `5 packets transmitted, 5 packets received, 0.00% packet loss`.
static COUNTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?P<sent>\d+)\s+packets?\s+transmitted,\s*(?P<received>\d+)\s+(?:packets?\s+)?received")
        .expect("ping counts regex")
});

/// `0.00% packet loss`, `100% packet loss`.
static LOSS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<loss>\d+(?:\.\d+)?)%\s*packet\s*loss").expect("loss regex"));

/// `round-trip min/avg/max = 0.412/0.501/0.688 ms` and the `rtt` spelling.
static TIMINGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:round-trip|rtt)[^=]*=\s*(?P<min>\d+(?:\.\d+)?)/(?P<avg>\d+(?:\.\d+)?)/(?P<max>\d+(?:\.\d+)?)",
    )
    .expect("timings regex")
});

/// Reads a ping summary.
///
/// Fails when there is no summary at all: a ping whose output we cannot read is
/// not a ping with zero replies.
pub fn parse(raw: &str) -> Result<PingResult, DriverError> {
    let counts = COUNTS.captures(raw).ok_or_else(|| {
        DriverError::ParseError("no packet counts in the ping output".to_string())
    })?;

    let packets_sent = counts["sent"].parse().unwrap_or(0);
    let packets_received = counts["received"].parse().unwrap_or(0);

    let packet_loss_percent = LOSS
        .captures(raw)
        .and_then(|captures| captures["loss"].parse().ok())
        .unwrap_or_else(|| {
            // Some platforms omit the loss figure. Deriving it from the counts
            // is arithmetic on what the router reported, not a guess.
            if packets_sent == 0 {
                100.0
            } else {
                100.0 - (f64::from(packets_received) / f64::from(packets_sent)) * 100.0
            }
        });

    let timings = TIMINGS.captures(raw);
    let (min_rtt_ms, avg_rtt_ms, max_rtt_ms) = match timings {
        Some(captures) => (
            captures["min"].parse().ok(),
            captures["avg"].parse().ok(),
            captures["max"].parse().ok(),
        ),
        // No round-trip line, which is what a total loss looks like. Reporting
        // zero would claim a reply that never came.
        None => (None, None, None),
    };

    Ok(PingResult {
        packets_sent,
        packets_received,
        packet_loss_percent,
        min_rtt_ms,
        avg_rtt_ms,
        max_rtt_ms,
        raw_output: raw.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_successful_ping() {
        let raw = "\
PING 198.51.100.1 56 data bytes
64 bytes from 198.51.100.1: icmp_seq=1 ttl=58 time=0.412 ms
--- 198.51.100.1 ping statistics ---
5 packets transmitted, 5 packets received, 0.00% packet loss
round-trip min/avg/max = 0.412/0.501/0.688 ms
";
        let result = parse(raw).unwrap();
        assert_eq!(result.packets_sent, 5);
        assert_eq!(result.packets_received, 5);
        assert_eq!(result.packet_loss_percent, 0.0);
        assert_eq!(result.avg_rtt_ms, Some(0.501));
    }

    /// A total loss prints no round-trip line. Reporting 0 ms would claim a
    /// reply that never arrived.
    #[test]
    fn a_total_loss_has_no_timings() {
        let raw = "\
--- 198.51.100.1 ping statistics ---
5 packets transmitted, 0 packets received, 100% packet loss
";
        let result = parse(raw).unwrap();
        assert_eq!(result.packets_received, 0);
        assert_eq!(result.packet_loss_percent, 100.0);
        assert_eq!(result.min_rtt_ms, None);
        assert_eq!(result.avg_rtt_ms, None);
        assert_eq!(result.max_rtt_ms, None);
    }

    #[test]
    fn partial_loss_is_read_from_the_router_not_recomputed() {
        let raw = "\
5 packets transmitted, 3 packets received, 40% packet loss
round-trip min/avg/max = 1.0/2.0/3.0 ms
";
        let result = parse(raw).unwrap();
        assert_eq!(result.packets_received, 3);
        assert_eq!(result.packet_loss_percent, 40.0);
    }

    #[test]
    fn the_rtt_spelling_is_read_too() {
        let raw = "\
5 packets transmitted, 5 received, 0% packet loss, time 4005ms
rtt min/avg/max/mdev = 0.412/0.501/0.688/0.098 ms
";
        let result = parse(raw).unwrap();
        assert_eq!(result.packets_received, 5);
        assert_eq!(result.max_rtt_ms, Some(0.688));
    }

    #[test]
    fn a_loss_figure_that_is_absent_is_derived_from_the_counts() {
        let raw = "4 packets transmitted, 2 packets received\n";
        let result = parse(raw).unwrap();
        assert_eq!(result.packet_loss_percent, 50.0);
    }

    /// Output with no summary is a parse failure, not a ping with no replies.
    #[test]
    fn output_without_a_summary_is_an_error() {
        let error = parse("ping: unknown host\n").unwrap_err();
        assert!(matches!(error, DriverError::ParseError(_)), "{error:?}");
    }
}
