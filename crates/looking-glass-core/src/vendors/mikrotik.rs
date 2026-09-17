use crate::driver::{QueryTarget, VendorDriver};
use std::net::IpAddr;

/// Driver para MikroTik RouterOS v6 e v7
/// Portado das diretrizes do Netmiko `mikrotik.py`
pub struct MikrotikDriver {
    pub is_v7: bool,
}

impl MikrotikDriver {
    pub fn new(is_v7: bool) -> Self {
        Self { is_v7 }
    }
}

impl VendorDriver for MikrotikDriver {
    fn vendor_name(&self) -> &'static str {
        if self.is_v7 {
            "mikrotik_routeros_v7"
        } else {
            "mikrotik_routeros_v6"
        }
    }

    /// No MikroTik, comandos executados no modo direto não paginam
    fn disable_paging_cmd(&self) -> Option<&'static str> {
        None
    }

    /// Regex do Netmiko para MikroTik: `r"\[[\w\.\-]+@[\w\.\-]+\]\s*[>#]"`
    fn prompt_pattern(&self) -> &'static str {
        r"\[[\w\.\-]+@[\w\.\-]+\]\s*[>#]"
    }

    fn format_ping(&self, target: &IpAddr, count: u8) -> String {
        format!("/ping address={} count={}", target, count.clamp(1, 20))
    }

    fn format_traceroute(&self, target: &IpAddr) -> String {
        format!("/tool traceroute address={} count=1 use-dns=no", target)
    }

    fn format_bgp_route(&self, target: &QueryTarget) -> String {
        if self.is_v7 {
            match target {
                QueryTarget::Ip(ip) => format!("/routing/bgp/route/print detail where dst={}", ip),
                QueryTarget::Prefix(net) => format!("/routing/bgp/route/print detail where dst={}", net),
                QueryTarget::Asn(asn) => format!("/routing/bgp/route/print detail where as-path~\"{}\"", asn),
            }
        } else {
            match target {
                QueryTarget::Ip(ip) => format!("/routing bgp advertisements print detail where dst-address={}", ip),
                QueryTarget::Prefix(net) => format!("/routing bgp advertisements print detail where dst-address={}", net),
                QueryTarget::Asn(asn) => format!("/routing bgp advertisements print detail where as-path~\"{}\"", asn),
            }
        }
    }
}
