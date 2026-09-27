use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DriverError {
    #[error("SSH connection failed: {0}")]
    ConnectionFailed(String),
    #[error("command timed out after {0} seconds")]
    Timeout(u64),
    #[error("protocol or I/O error: {0}")]
    IoError(String),
    #[error("invalid target for this command: {0}")]
    InvalidTarget(String),
    #[error("empty or truncated response from the router")]
    EmptyResponse,
    #[error("could not parse the response: {0}")]
    ParseError(String),
    #[error("JSON deserialization error: {0}")]
    JsonError(#[from] serde_json::Error),
    /// The driver has no parser for this query yet.
    ///
    /// A missing parser MUST surface as this error. Returning an empty or
    /// default-valued result would be indistinguishable from a real answer,
    /// and a diagnostic tool that invents data is worse than one that is
    /// missing (ADR-0006).
    #[error("{vendor} cannot answer {query} yet: no parser implemented")]
    Unsupported {
        vendor: &'static str,
        query: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryType {
    Ping,
    Traceroute,
    BgpRoute,
    BgpAspath,
    BgpSummary,
}

/// What a visitor asked about.
///
/// Build one with [`crate::target::parse_target`] — it is the only validated
/// path from text to this type. Constructing a variant directly is fine for
/// values the program already holds as typed data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryTarget {
    Ip(IpAddr),
    Prefix(IpNet),
    Asn(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpkiStatus {
    Valid,
    Invalid,
    NotFound,
    NotChecked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    pub packets_sent: u32,
    pub packets_received: u32,
    pub packet_loss_percent: f64,
    pub min_rtt_ms: Option<f64>,
    pub avg_rtt_ms: Option<f64>,
    pub max_rtt_ms: Option<f64>,
    pub raw_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracerouteHop {
    pub hop: u32,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    pub rtt_ms: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracerouteResult {
    pub target: String,
    pub hops: Vec<TracerouteHop>,
    pub raw_output: String,
}

/// Where an RPKI validation state came from.
///
/// Operators need to tell "my router validated this" apart from "we asked a
/// third party", because the two say different things about their own filtering
/// (ADR-0010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpkiSource {
    /// Reported by the queried router itself.
    Router,
    /// A validator the operator configured, such as Routinator.
    LocalValidator,
    /// Public RIPEstat lookup.
    RipeStat,
    /// Nobody validated it.
    None,
}

/// RPKI state plus its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpkiValidation {
    pub status: RpkiStatus,
    pub source: RpkiSource,
}

impl Default for RpkiValidation {
    fn default() -> Self {
        Self {
            status: RpkiStatus::NotChecked,
            source: RpkiSource::None,
        }
    }
}

impl RpkiValidation {
    /// State the router itself reported.
    pub fn from_router(status: RpkiStatus) -> Self {
        Self {
            status,
            source: RpkiSource::Router,
        }
    }
}

/// BGP origin attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Igp,
    Egp,
    Incomplete,
}

impl Origin {
    /// Reads the trailing marker vendors print after the AS path: `i`, `e`
    /// or `?`. Returns `None` for anything else, so an absent marker stays
    /// absent instead of defaulting to IGP.
    pub fn from_marker(marker: char) -> Option<Self> {
        match marker {
            'i' | 'I' => Some(Self::Igp),
            'e' | 'E' => Some(Self::Egp),
            '?' => Some(Self::Incomplete),
            _ => None,
        }
    }
}

/// Which BGP community encoding a value uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunityKind {
    /// RFC 1997, `asn:value`.
    Standard,
    /// RFC 8092, `asn:value:value`.
    Large,
    /// RFC 4360, printed by vendors in several shapes.
    Extended,
    /// Recognised by name, such as `no-export`.
    WellKnown,
}

/// One community, kept in the exact form the router printed plus what we could
/// make of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Community {
    /// Verbatim, as the router printed it. Always present.
    pub raw: String,
    pub kind: CommunityKind,
    /// Human name for well-known values. `None` means we do not know what the
    /// operator's policy assigns to it — it is not decoded, and MUST NOT be
    /// presented as if it were.
    pub name: Option<String>,
}

impl Community {
    /// Classifies a community string without inventing meaning for it.
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let well_known = match trimmed {
            "no-export" | "noexport" | "65535:65281" => Some("no-export"),
            "no-advertise" | "noadvertise" | "65535:65282" => Some("no-advertise"),
            "no-export-subconfed" | "65535:65283" => Some("no-export-subconfed"),
            "blackhole" | "65535:666" => Some("blackhole"),
            "graceful-shutdown" | "65535:0" => Some("graceful-shutdown"),
            _ => None,
        };

        if let Some(name) = well_known {
            return Self {
                raw: trimmed.to_string(),
                kind: CommunityKind::WellKnown,
                name: Some(name.to_string()),
            };
        }

        let parts: Vec<&str> = trimmed.split(':').collect();
        let all_numeric = parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        let kind = match parts.len() {
            2 if all_numeric => CommunityKind::Standard,
            3 if all_numeric => CommunityKind::Large,
            _ => CommunityKind::Extended,
        };

        Self {
            raw: trimmed.to_string(),
            kind,
            name: None,
        }
    }
}

/// One path towards a prefix, normalised across vendors (ADR-0006).
///
/// Every optional field means the same thing everywhere: the router did not
/// report it. A driver MUST leave it `None` rather than supply a plausible
/// value.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BgpPath {
    /// The prefix this path is for. `None` when the vendor output could not be
    /// parsed into a network, which happens on malformed or truncated output.
    pub prefix: Option<IpNet>,
    pub next_hop: Option<IpAddr>,
    /// The peer that advertised this path, when the vendor prints it.
    pub peer: Option<IpAddr>,
    /// The router selected this path as best.
    pub is_best: bool,
    /// The router considers the path valid (the `*` flag in most CLIs).
    pub is_valid: Option<bool>,
    pub as_path: Vec<u32>,
    pub local_pref: Option<u32>,
    pub med: Option<u32>,
    pub weight: Option<u32>,
    pub origin: Option<Origin>,
    pub communities: Vec<Community>,
    pub rpki: RpkiValidation,
}

impl BgpPath {
    /// Origin AS: the last entry of the AS path.
    ///
    /// Derived, not invented — it is what the path says. `None` for an empty
    /// AS path, which is what a locally originated route looks like.
    pub fn origin_as(&self) -> Option<u32> {
        self.as_path.last().copied()
    }
}

/// Parses a prefix as printed by a router, accepting a bare address as a host
/// route. Returns `None` when the text is not a network, so the caller records
/// "the router printed something we could not read" instead of guessing.
pub fn parse_network(text: &str) -> Option<IpNet> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(net) = text.parse::<IpNet>() {
        return Some(net);
    }
    // A bare address is a host route: /32 or /128.
    text.parse::<IpAddr>().ok().and_then(|ip| {
        let bits = if ip.is_ipv4() { 32 } else { 128 };
        IpNet::new(ip, bits).ok()
    })
}

/// Parses a next hop or peer address, returning `None` for anything that is not
/// an address — including vendor placeholders such as `0.0.0.0` for locally
/// originated routes, which is not a reachable next hop.
pub fn parse_hop(text: &str) -> Option<IpAddr> {
    let ip: IpAddr = text.trim().parse().ok()?;
    if ip.is_unspecified() {
        None
    } else {
        Some(ip)
    }
}

/// How much of the router output the driver managed to read.
///
/// An empty `paths` list with `Complete` means "this router has no route for
/// that prefix", which is a real answer. It MUST NOT double as "the parser
/// failed" (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum Completeness {
    /// Everything the router printed was understood.
    Complete,
    /// Some of the output was not understood. The raw output is still there, so
    /// the interface can show it and say that the parsed view is partial.
    Partial { unreadable_lines: usize },
}

/// The answer to one BGP route query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BgpRouteResult {
    pub paths: Vec<BgpPath>,
    /// What the router printed, always preserved (ADR-0006).
    pub raw_output: String,
    pub completeness: Completeness,
    /// Set when the executor cut the output at the configured cap.
    pub truncated: bool,
}

impl BgpRouteResult {
    pub fn new(paths: Vec<BgpPath>, raw_output: impl Into<String>) -> Self {
        Self {
            paths,
            raw_output: raw_output.into(),
            completeness: Completeness::Complete,
            truncated: false,
        }
    }

    /// Marks the result as partially understood.
    pub fn partial(mut self, unreadable_lines: usize) -> Self {
        if unreadable_lines > 0 {
            self.completeness = Completeness::Partial { unreadable_lines };
        }
        self
    }

    /// The best path, when the router marked one.
    pub fn best(&self) -> Option<&BgpPath> {
        self.paths.iter().find(|p| p.is_best)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BgpPeerSummary {
    pub peer_ip: String,
    pub peer_as: u32,
    pub state: String,
    pub uptime: String,
    pub prefixes_received: u32,
    pub prefixes_accepted: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BgpSummaryResult {
    pub router_id: Option<String>,
    pub local_as: Option<u32>,
    pub peers: Vec<BgpPeerSummary>,
    pub raw_output: String,
}

/// Contract every vendor driver implements: build read-only commands and parse
/// their output into the normalised model.
pub trait VendorDriver: Send + Sync {
    /// Identifier of the network operating system this driver speaks. It MUST
    /// match a key in the command catalogue, which owns the commands, the
    /// paging behaviour and the prompt pattern for this vendor.
    fn vendor_name(&self) -> &'static str;

    // --- Parsers. A parser that cannot read the output returns an error. ---
    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError>;
    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError>;
    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError>;
    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError>;
}
