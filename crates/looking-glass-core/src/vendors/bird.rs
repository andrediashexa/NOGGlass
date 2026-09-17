use crate::driver::{QueryTarget, VendorDriver};
use std::net::IpAddr;

/// Driver para BIRD 2 (Internet Routing Daemon)
/// Suporta execução via SSH ou CLI local (birdc)
pub struct BirdDriver;

impl VendorDriver for BirdDriver {
    fn vendor_name(&self) -> &'static str {
        "bird_routing_daemon"
    }

    /// O BIRD não pagina comandos quando executados de forma não interativa
    fn disable_paging_cmd(&self) -> Option<&'static str> {
        None
    }

    /// Prompt padrão do birdc: `r"bird>\s*$"`
    fn prompt_pattern(&self) -> &'static str {
        r"bird>\s*$"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        // BIRD é daemon de roteamento; ping é executado pelo shell host
        format!("ping -c {} {}", count.clamp(1, 20), target)
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("traceroute -n {}", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        match target {
            QueryTarget::Ip(ip) => format!("show route for {} all", ip),
            QueryTarget::Prefix(net) => format!("show route for {} all", net),
            QueryTarget::Asn(asn) => format!("show route where bgp_path ~ [= * {} * =] all", asn),
        }
    }
}
