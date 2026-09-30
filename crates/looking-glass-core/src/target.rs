//! Parsing and validation of what a visitor asks about.
//!
//! This module is the security boundary of the whole product. Everything a
//! visitor types enters here and leaves as a typed value or an error — never as
//! a string that a driver could interpolate into a router command line.
//!
//! The rules it enforces are deliberately strict, because the blast radius is a
//! production router:
//!
//! * input is length-capped before anything else looks at it;
//! * an address, a prefix or an AS number is parsed by the standard library or
//!   by `ipnet`, never by a hand-written scanner;
//! * a prefix MUST be written in canonical form — host bits set is a typo, and
//!   silently masking it would answer a question nobody asked;
//! * documentation, loopback, link-local, multicast and unspecified ranges are
//!   refused, because asking a production router about them is never a real
//!   diagnostic and is a common probing pattern.

use crate::driver::QueryTarget;
use ipnet::IpNet;
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

/// Longest accepted input, in bytes.
///
/// An IPv6 prefix in its longest legal form is well under 50 bytes. The cap
/// exists so a megabyte of text is rejected before any parser allocates.
pub const MAX_TARGET_LEN: usize = 64;

/// Shortest IPv4 prefix a visitor may ask about.
///
/// Anything shorter is a full-table walk on the router for no diagnostic gain.
pub const MIN_IPV4_PREFIX_LEN: u8 = 8;

/// Shortest IPv6 prefix a visitor may ask about.
pub const MIN_IPV6_PREFIX_LEN: u8 = 16;

/// Why a target was refused.
///
/// The message is shown to the visitor, so it says what is wrong without
/// describing internal structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetError {
    Empty,
    TooLong { len: usize, max: usize },
    NotRoutable(&'static str),
    PrefixTooShort { len: u8, min: u8 },
    HostBitsSet { canonical: String },
    AsnOutOfRange,
    Unparseable,
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "enter an address, a prefix or an AS number"),
            Self::TooLong { len, max } => {
                write!(f, "input is {len} bytes long, the maximum is {max}")
            }
            Self::NotRoutable(what) => write!(f, "{what} addresses cannot be queried"),
            Self::PrefixTooShort { len, min } => write!(
                f,
                "/{len} is shorter than the smallest allowed prefix /{min}"
            ),
            Self::HostBitsSet { canonical } => {
                write!(f, "prefix has host bits set, did you mean {canonical}?")
            }
            Self::AsnOutOfRange => write!(f, "AS number is out of range"),
            Self::Unparseable => write!(
                f,
                "not a valid IPv4/IPv6 address, prefix (198.51.100.0/24) or AS number (AS65500)"
            ),
        }
    }
}

impl std::error::Error for TargetError {}

/// Refuses addresses that no visitor has a legitimate reason to trace from a
/// production router.
fn reject_special(ip: IpAddr) -> Result<(), TargetError> {
    let reason = match ip {
        IpAddr::V4(v4) => {
            if v4.is_unspecified() {
                Some("unspecified")
            } else if v4.is_loopback() {
                Some("loopback")
            } else if v4.is_link_local() {
                Some("link-local")
            } else if v4.is_broadcast() {
                Some("broadcast")
            } else if v4.is_multicast() {
                Some("multicast")
            } else if v4.octets()[0] >= 240 {
                // 240.0.0.0/4, reserved by RFC 1112.
                Some("reserved")
            } else {
                None
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() {
                Some("unspecified")
            } else if v6.is_loopback() {
                Some("loopback")
            } else if v6.is_multicast() {
                Some("multicast")
            } else if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                Some("link-local")
            } else {
                None
            }
        }
    };

    match reason {
        Some(what) => Err(TargetError::NotRoutable(what)),
        None => Ok(()),
    }
}

/// Parses what the visitor typed into a typed target.
///
/// This is the only supported way to build a [`QueryTarget`] from text.
///
/// ```
/// use looking_glass_core::target::parse_target;
/// use looking_glass_core::driver::QueryTarget;
///
/// assert!(matches!(
///     parse_target("198.51.100.0/24").unwrap(),
///     QueryTarget::Prefix(_)
/// ));
/// assert!(matches!(parse_target("AS65500").unwrap(), QueryTarget::Asn(65500)));
/// assert!(parse_target("198.51.100.1; reload").is_err());
/// ```
pub fn parse_target(input: &str) -> Result<QueryTarget, TargetError> {
    let raw = input.trim();

    if raw.is_empty() {
        return Err(TargetError::Empty);
    }
    if raw.len() > MAX_TARGET_LEN {
        return Err(TargetError::TooLong {
            len: raw.len(),
            max: MAX_TARGET_LEN,
        });
    }

    // AS numbers, written as "AS65500", "as65500" or "65500". A bare number is
    // only an AS number when it cannot be anything else, which is why this runs
    // after the address parsers below would have failed.
    if let Some(digits) = raw
        .strip_prefix("AS")
        .or_else(|| raw.strip_prefix("as"))
        .or_else(|| raw.strip_prefix("As"))
    {
        return parse_asn(digits);
    }

    if let Ok(ip) = IpAddr::from_str(raw) {
        reject_special(ip)?;
        return Ok(QueryTarget::Ip(ip));
    }

    if raw.contains('/') {
        return parse_prefix(raw);
    }

    // A bare number that is not an address is an AS number.
    if raw.bytes().all(|b| b.is_ascii_digit()) {
        return parse_asn(raw);
    }

    Err(TargetError::Unparseable)
}

fn parse_asn(digits: &str) -> Result<QueryTarget, TargetError> {
    let digits = digits.trim();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(TargetError::Unparseable);
    }
    // AS 0 and AS 23456 (the 4-byte transition placeholder) are never a real
    // query, and u32::MAX is reserved.
    match digits.parse::<u32>() {
        Ok(0) | Ok(23456) | Ok(u32::MAX) => Err(TargetError::AsnOutOfRange),
        Ok(asn) => Ok(QueryTarget::Asn(asn)),
        Err(_) => Err(TargetError::AsnOutOfRange),
    }
}

fn parse_prefix(raw: &str) -> Result<QueryTarget, TargetError> {
    let net = IpNet::from_str(raw).map_err(|_| TargetError::Unparseable)?;

    // "198.51.100.5/24" is a typo. Answering about 198.51.100.0/24 instead
    // would silently answer a different question, so say so and show the form
    // that was probably meant.
    if net.addr() != net.network() {
        return Err(TargetError::HostBitsSet {
            canonical: net.trunc().to_string(),
        });
    }

    reject_special(net.addr())?;

    let (len, min) = match net {
        IpNet::V4(v4) => (v4.prefix_len(), MIN_IPV4_PREFIX_LEN),
        IpNet::V6(v6) => (v6.prefix_len(), MIN_IPV6_PREFIX_LEN),
    };
    if len < min {
        return Err(TargetError::PrefixTooShort { len, min });
    }

    Ok(QueryTarget::Prefix(net))
}

impl FromStr for QueryTarget {
    type Err = TargetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_target(s)
    }
}

/// Limits applied to one query, so a single visitor cannot hold a router.
///
/// Defaults are conservative on purpose: an operator raises them knowingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryLimits {
    /// Wall-clock budget for the whole command.
    pub timeout_secs: u64,
    /// Maximum bytes of router output kept. Output past this is dropped and the
    /// result is flagged as truncated.
    pub max_output_bytes: usize,
    /// Ping probes. Clamped by the drivers as well.
    pub ping_count: u8,
}

impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            // A full BGP table dump is tens of megabytes; a legitimate answer to
            // an ASN or prefix query with multiple paths/communities fits in 2 MB,
            // matching the SSH transport buffer ceiling.
            max_output_bytes: 2 * 1024 * 1024,
            ping_count: 5,
        }
    }
}

impl QueryLimits {
    /// Truncates output to the configured cap.
    ///
    /// Returns the kept output and whether anything was dropped, so the caller
    /// can tell the visitor that the answer is partial. The cut lands on a
    /// character boundary, never mid-UTF-8.
    pub fn truncate(&self, output: &str) -> (String, bool) {
        if output.len() <= self.max_output_bytes {
            return (output.to_string(), false);
        }
        let mut end = self.max_output_bytes;
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        (output[..end].to_string(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_addresses_prefixes_and_as_numbers() {
        assert!(matches!(
            parse_target("198.51.100.1").unwrap(),
            QueryTarget::Ip(_)
        ));
        assert!(matches!(
            parse_target("2001:db8::1").unwrap(),
            QueryTarget::Ip(_)
        ));
        assert!(matches!(
            parse_target("198.51.100.0/24").unwrap(),
            QueryTarget::Prefix(_)
        ));
        assert!(matches!(
            parse_target("2001:db8::/32").unwrap(),
            QueryTarget::Prefix(_)
        ));
        assert_eq!(parse_target("AS65500"), Ok(QueryTarget::Asn(65500)));
        assert_eq!(parse_target("as65500"), Ok(QueryTarget::Asn(65500)));
        assert_eq!(parse_target("65500"), Ok(QueryTarget::Asn(65500)));
        assert!(parse_target("  198.51.100.1  ").is_ok());
    }

    /// The property this whole module exists for: nothing that could change the
    /// meaning of a router command line survives parsing.
    #[test]
    fn rejects_anything_that_could_reach_a_command_line() {
        for hostile in [
            "198.51.100.1; reload",
            "198.51.100.1 && show run",
            "198.51.100.1 | display set",
            "198.51.100.1\nwrite memory",
            "$(reboot)",
            "`id`",
            "198.51.100.1 -c 99999",
            "--count=99999",
            "198.51.100.1/24 ; delete",
            "AS65500; quit",
            "' or '1'='1",
            "../../etc/passwd",
            "198.51.100.1%eth0",
        ] {
            assert!(
                parse_target(hostile).is_err(),
                "accepted hostile input: {hostile:?}"
            );
        }
    }

    #[test]
    fn rejects_oversized_input() {
        let long = "1".repeat(MAX_TARGET_LEN + 1);
        assert_eq!(
            parse_target(&long),
            Err(TargetError::TooLong {
                len: MAX_TARGET_LEN + 1,
                max: MAX_TARGET_LEN
            })
        );
    }

    #[test]
    fn rejects_special_use_addresses() {
        for special in [
            "127.0.0.1",
            "0.0.0.0",
            "169.254.1.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "ff02::1",
        ] {
            assert!(
                matches!(parse_target(special), Err(TargetError::NotRoutable(_))),
                "accepted special-use address: {special}"
            );
        }
    }

    #[test]
    fn rejects_prefixes_with_host_bits_set() {
        match parse_target("198.51.100.5/24") {
            Err(TargetError::HostBitsSet { canonical }) => {
                assert_eq!(canonical, "198.51.100.0/24");
            }
            other => panic!("expected a host-bits error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_prefixes_that_would_walk_the_table() {
        assert_eq!(
            parse_target("10.0.0.0/7"),
            Err(TargetError::PrefixTooShort { len: 7, min: 8 })
        );
        // Written canonically, so it fails on length rather than on host bits.
        assert_eq!(
            parse_target("2000::/8"),
            Err(TargetError::PrefixTooShort { len: 8, min: 16 })
        );
    }

    #[test]
    fn rejects_reserved_as_numbers() {
        assert_eq!(parse_target("AS0"), Err(TargetError::AsnOutOfRange));
        assert_eq!(parse_target("AS23456"), Err(TargetError::AsnOutOfRange));
        assert_eq!(
            parse_target("AS4294967296"),
            Err(TargetError::AsnOutOfRange)
        );
    }

    #[test]
    fn empty_input_says_what_to_type() {
        assert_eq!(parse_target("   "), Err(TargetError::Empty));
    }

    #[test]
    fn truncation_flags_partial_output_and_keeps_utf8_intact() {
        let limits = QueryLimits {
            max_output_bytes: 8,
            ..QueryLimits::default()
        };

        let (kept, truncated) = limits.truncate("short");
        assert_eq!(kept, "short");
        assert!(!truncated);

        let (kept, truncated) = limits.truncate("0123456789");
        assert_eq!(kept, "01234567");
        assert!(truncated);

        // Multi-byte characters appear in router banners; cutting mid-character
        // would produce invalid UTF-8.
        let (kept, truncated) = limits.truncate("aaaaaaaç");
        assert!(truncated);
        assert_eq!(kept, "aaaaaaa");
    }
}
