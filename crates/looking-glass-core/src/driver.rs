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
pub enum QueryType {
    Ping,
    Traceroute,
    BgpRoute,
    BgpSummary,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BgpPath {
    pub is_best: bool,
    pub network: String,
    pub next_hop: String,
    pub as_path: Vec<u32>,
    pub local_pref: Option<u32>,
    pub med: Option<u32>,
    pub weight: Option<u32>,
    /// BGP origin attribute as reported by the router: IGP, EGP or incomplete.
    /// `None` when the vendor output does not carry it — never defaulted.
    pub origin: Option<String>,
    pub communities: Vec<String>,
    pub rpki_status: RpkiStatus,
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
    /// Identifier of the network operating system this driver speaks.
    fn vendor_name(&self) -> &'static str;

    /// Command that disables output paging (the `--More--` prompt), if any.
    fn disable_paging_cmd(&self) -> Option<&'static str>;

    /// Regex matching the router prompt, used to detect end of output.
    fn prompt_pattern(&self) -> &'static str;

    // --- Command builders. Arguments are typed, never raw user text. ---
    fn format_ping(&self, target: &IpAddr, count: u8) -> String;
    fn format_traceroute(&self, target: &IpAddr) -> String;
    fn format_bgp_route(&self, target: &QueryTarget) -> String;
    fn format_bgp_summary(&self) -> String;

    // --- Parsers. A parser that cannot read the output returns an error. ---
    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError>;
    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError>;
    fn parse_bgp_route(&self, raw: &str) -> Result<Vec<BgpPath>, DriverError>;
    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError>;
}
