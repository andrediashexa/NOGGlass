use crate::driver::{QueryTarget, VendorDriver};
use std::net::IpAddr;

/// Driver para Juniper Junos (MX, PTX, QFX, SRX)
/// Portado das diretrizes do Netmiko `juniper.py` com suporte a JSON nativo
pub struct JuniperDriver;

impl VendorDriver for JuniperDriver {
    fn vendor_name(&self) -> &'static str {
        "juniper_junos"
    }

    /// Netmiko: `set cli screen-length 0`
    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("set cli screen-length 0")
    }

    /// Regex do Netmiko para Junos: `r"[\w\.\-]+[>#%]\s*$"`
    fn prompt_pattern(&self) -> &'static str {
        r"[\w\.\-]+[>#%]\s*$"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping {} count {} no-resolve", target, count.clamp(1, 20))
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("traceroute {} no-resolve", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        // Junos suporta "| no-more" e "| display json"
        match target {
            QueryTarget::Ip(ip) => format!("show route {} detail | no-more", ip),
            QueryTarget::Prefix(net) => format!("show route {} detail | no-more", net),
            QueryTarget::Asn(asn) => format!("show route aspath-regex \".*{}.*\" detail | no-more", asn),
        }
    }
}
