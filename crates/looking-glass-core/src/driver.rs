use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DriverError {
    #[error("Falha de conexão SSH: {0}")]
    ConnectionFailed(String),
    #[error("Timeout na execução do comando após {0} segundos")]
    Timeout(u64),
    #[error("Erro de protocolo ou I/O: {0}")]
    IoError(String),
    #[error("Target inválido para o comando: {0}")]
    InvalidTarget(String),
    #[error("Resposta vazia ou truncada do roteador")]
    EmptyResponse,
    #[error("Falha no parsing da resposta: {0}")]
    ParseError(String),
    #[error("Erro de deserialização JSON: {0}")]
    JsonError(#[from] serde_json::Error),
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
    pub origin: String,
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

/// Contrato universal de Driver de Roteador com suporte a comandos e parsing
pub trait VendorDriver: Send + Sync {
    /// Nome identificador do vendor
    fn vendor_name(&self) -> &'static str;

    /// Comando de desativação de paginação (--More--)
    fn disable_paging_cmd(&self) -> Option<&'static str>;

    /// Regex de retorno do prompt do roteador
    fn prompt_pattern(&self) -> &'static str;

    // --- Formatadores de comandos ---
    fn format_ping(&self, target: &IpAddr, count: u8) -> String;
    fn format_traceroute(&self, target: &IpAddr) -> String;
    fn format_bgp_route(&self, target: &QueryTarget) -> String;
    fn format_bgp_summary(&self) -> String;

    // --- Parsers estruturados ---
    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError>;
    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError>;
    fn parse_bgp_route(&self, raw: &str) -> Result<Vec<BgpPath>, DriverError>;
    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError>;
}
