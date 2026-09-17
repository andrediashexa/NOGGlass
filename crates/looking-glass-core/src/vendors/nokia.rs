use crate::driver::{QueryTarget, VendorDriver};
use std::net::IpAddr;

/// Driver para Nokia SR OS (TiMOS)
/// Portado das diretrizes do Netmiko `nokia_sros.py`
pub struct NokiaSrosDriver;

impl VendorDriver for NokiaSrosDriver {
    fn vendor_name(&self) -> &'static str {
        "nokia_sros"
    }

    /// Netmiko: `environment no more`
    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("environment no more")
    }

    /// Prompt do Nokia SR OS: `r"(\*?[AB]:[\w\.\-]+[>#]\s*$)"`
    fn prompt_pattern(&self) -> &'static str {
        r"(\*?[AB]:[\w\.\-]+[>#]\s*$)"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping {} count {}", target, count.clamp(1, 20))
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("traceroute {}", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        match target {
            QueryTarget::Ip(ip) => format!("show router bgp routes {}", ip),
            QueryTarget::Prefix(net) => format!("show router bgp routes {}", net),
            QueryTarget::Asn(asn) => format!("show router bgp routes aspath-regex \".*{}.*\"", asn),
        }
    }
}
