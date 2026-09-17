pub mod driver;
pub mod vendors;

pub use driver::{DriverError, QueryTarget, QueryType, VendorDriver};
pub use vendors::*;

use regex::Regex;
use std::sync::LazyLock;

/// Regex universal de detecção de término de comando portado do Netmiko
pub static DEFAULT_BASE_PROMPT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\>#\]]\s*$").expect("Regex inválido de prompt padrão")
});

#[cfg(test)]
mod tests {
    use super::*;
    use ipnet::IpNet;
    use std::net::IpAddr;
    use std::str::FromStr;

    #[test]
    fn test_huawei_commands() {
        let driver = HuaweiVrpDriver;
        assert_eq!(driver.disable_paging_cmd(), Some("screen-length 0 temporary"));
        
        let ip = IpAddr::from_str("192.0.2.1").unwrap();
        assert_eq!(driver.format_ping(&ip, 5), "ping -c 5 192.0.2.1");

        let net = IpNet::from_str("198.51.100.0/24").unwrap();
        assert_eq!(
            driver.format_bgp_route(&QueryTarget::Prefix(net)),
            "display bgp routing-table 198.51.100.0 255.255.255.0"
        );
    }

    #[test]
    fn test_mikrotik_v7_commands() {
        let driver = MikrotikDriver::new(true);
        let net = IpNet::from_str("198.51.100.0/24").unwrap();
        assert_eq!(
            driver.format_bgp_route(&QueryTarget::Prefix(net)),
            "/routing/bgp/route/print detail where dst=198.51.100.0/24"
        );
    }

    #[test]
    fn test_cisco_xr_commands() {
        let driver = CiscoDriver::new(true);
        assert_eq!(driver.disable_paging_cmd(), Some("terminal length 0"));
        let net = IpNet::from_str("198.51.100.0/24").unwrap();
        assert_eq!(
            driver.format_bgp_route(&QueryTarget::Prefix(net)),
            "show bgp ipv4 unicast 198.51.100.0/24"
        );
    }

    #[test]
    fn test_juniper_commands() {
        let driver = JuniperDriver;
        assert_eq!(driver.disable_paging_cmd(), Some("set cli screen-length 0"));
        let ip = IpAddr::from_str("192.0.2.1").unwrap();
        assert_eq!(
            driver.format_bgp_route(&QueryTarget::Ip(ip)),
            "show route 192.0.2.1 detail | no-more"
        );
    }
}
