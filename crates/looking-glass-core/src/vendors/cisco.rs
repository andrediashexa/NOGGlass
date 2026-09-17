use crate::driver::{QueryTarget, VendorDriver};
use std::net::IpAddr;

/// Driver para Cisco IOS-XR (ASR9000, NCS5500, 8000) e IOS-XE (ASR1000, Catalyst)
/// Portado das diretrizes do Netmiko `cisco_xr.py` e `cisco_ios.py`
pub struct CiscoDriver {
    pub is_xr: bool,
}

impl CiscoDriver {
    pub fn new(is_xr: bool) -> Self {
        Self { is_xr }
    }
}

impl VendorDriver for CiscoDriver {
    fn vendor_name(&self) -> &'static str {
        if self.is_xr {
            "cisco_iosxr"
        } else {
            "cisco_iosxe"
        }
    }

    /// Netmiko: `terminal length 0`
    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("terminal length 0")
    }

    /// Regex do Netmiko para Cisco: `r"[\w\.\-]+[>#]"`
    fn prompt_pattern(&self) -> &'static str {
        r"[\w\.\-]+[>#]"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("ping {} repeat {}", target, count.clamp(1, 20))
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("traceroute {}", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        if self.is_xr {
            match target {
                QueryTarget::Ip(ip) => {
                    if ip.is_ipv4() {
                        format!("show bgp ipv4 unicast {}", ip)
                    } else {
                        format!("show bgp ipv6 unicast {}", ip)
                    }
                }
                QueryTarget::Prefix(net) => {
                    if net.addr().is_ipv4() {
                        format!("show bgp ipv4 unicast {}", net)
                    } else {
                        format!("show bgp ipv6 unicast {}", net)
                    }
                }
                QueryTarget::Asn(asn) => format!("show bgp regexp _{}_", asn),
            }
        } else {
            match target {
                QueryTarget::Ip(ip) => format!("show ip bgp {}", ip),
                QueryTarget::Prefix(net) => format!("show ip bgp {}", net),
                QueryTarget::Asn(asn) => format!("show ip bgp regexp _{}_", asn),
            }
        }
    }
}
