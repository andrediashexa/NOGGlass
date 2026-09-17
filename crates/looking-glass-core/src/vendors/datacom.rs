use crate::driver::{QueryTarget, VendorDriver};
use std::net::IpAddr;

/// Driver para Datacom DmOS
/// Portado das diretrizes do Netmiko para switch/roteadores Datacom
pub struct DatacomDriver;

impl VendorDriver for DatacomDriver {
    fn vendor_name(&self) -> &'static str {
        "datacom_dmos"
    }

    /// Netmiko Datacom: `terminal length 0`
    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("terminal length 0")
    }

    /// Prompt Datacom DmOS
    fn prompt_pattern(&self) -> &'static str {
        r"[\w\.\-]+[>#]"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping {} count {}", target, count.clamp(1, 20))
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("traceroute {}", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        match target {
            QueryTarget::Ip(ip) => format!("show ip bgp {}", ip),
            QueryTarget::Prefix(net) => format!("show ip bgp {}", net),
            QueryTarget::Asn(asn) => format!("show ip bgp regexp {}", asn),
        }
    }
}
