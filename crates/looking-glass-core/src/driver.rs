use ipnet::IpNet;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Contrato universal de Driver de Roteador (Portado do modelo BaseConnection do Netmiko)
pub trait VendorDriver: Send + Sync {
    /// Nome identificador do vendor (ex: "mikrotik_routeros", "huawei_vrp")
    fn vendor_name(&self) -> &'static str;

    /// Comando enviado logo após o handshake para desabilitar quebra de tela (--More--)
    /// Equivalente ao `disable_paging()` do Netmiko
    fn disable_paging_cmd(&self) -> Option<&'static str>;

    /// Expressão regular padrão para detecção de retorno do prompt do roteador
    /// Equivalente ao `check_prompt()` do Netmiko
    fn prompt_pattern(&self) -> &'static str;

    /// Converte um Ping tipado para o comando específico do vendor
    fn format_ping(&self, target: &IpAddr, count: u8) -> String;

    /// Converte um Traceroute tipado para o comando específico do vendor
    fn format_traceroute(&self, target: &IpAddr) -> String;

    /// Converte uma consulta BGP tipada para o comando do vendor
    fn format_bgp_route(&self, target: &QueryTarget) -> String;
}
