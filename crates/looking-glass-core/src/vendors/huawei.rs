use crate::driver::{QueryTarget, VendorDriver};
use std::net::IpAddr;

/// Driver para Huawei VRP (NE40E, NE8000, S6730, etc.)
/// Portado das diretrizes do Netmiko `huawei.py`
pub struct HuaweiVrpDriver;

impl VendorDriver for HuaweiVrpDriver {
    fn vendor_name(&self) -> &'static str {
        "huawei_vrp"
    }

    /// Netmiko: `screen-length 0 temporary`
    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("screen-length 0 temporary")
    }

    /// Regex do Netmiko para Huawei: `r"<[\w\.\-]+>|\[[\w\.\-]+\]"`
    fn prompt_pattern(&self) -> &'static str {
        r"<[\w\.\-]+>|\[[\w\.\-]+\]"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping -c {} {}", count.clamp(1, 20), target)
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("tracert {}", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        match target {
            QueryTarget::Ip(ip) => {
                if ip.is_ipv4() {
                    format!("display bgp routing-table {}", ip)
                } else {
                    format!("display bgp ipv6 routing-table {}", ip)
                }
            }
            QueryTarget::Prefix(net) => {
                if net.addr().is_ipv4() {
                    format!("display bgp routing-table {} {}", net.addr(), net.netmask())
                } else {
                    format!("display bgp ipv6 routing-table {}/{}", net.addr(), net.prefix_len())
                }
            }
            QueryTarget::Asn(asn) => format!("display bgp routing-table as-path-filter {}", asn),
        }
    }
}
