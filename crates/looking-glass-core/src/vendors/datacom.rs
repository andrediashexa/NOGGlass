use crate::driver::{
    BgpRouteResult, BgpSummaryResult, DriverError, PingResult, QueryTarget, TracerouteResult,
    VendorDriver,
};
use std::net::IpAddr;

pub struct DatacomDriver;

impl VendorDriver for DatacomDriver {
    fn vendor_name(&self) -> &'static str {
        "datacom_dmos"
    }

    fn disable_paging_cmd(&self) -> Option<&'static str> {
        Some("terminal length 0")
    }

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

    fn format_bgp_summary(&self) -> String {
        "show ip bgp summary".to_string()
    }

    fn parse_ping(&self, _raw: &str) -> Result<PingResult, DriverError> {
        // Previously returned a hardcoded 5/5 with 0% loss for any input, so a
        // fully failing ping rendered as a perfect one. Until a real parser
        // exists, say so.
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "ping",
        })
    }

    fn parse_traceroute(&self, _raw: &str) -> Result<TracerouteResult, DriverError> {
        // No hop parser yet. An empty hop list would be indistinguishable from
        // a traceroute that legitimately returned nothing.
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "traceroute",
        })
    }

    fn parse_bgp_route(&self, _raw: &str) -> Result<BgpRouteResult, DriverError> {
        // An empty path list means "this router has no route for that prefix",
        // which is a real and useful answer. It MUST NOT double as "we did not
        // implement this".
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "bgp_route",
        })
    }

    fn parse_bgp_summary(&self, _raw: &str) -> Result<BgpSummaryResult, DriverError> {
        // An empty peer list would read as "this router has no BGP sessions".
        Err(DriverError::Unsupported {
            vendor: self.vendor_name(),
            query: "bgp_summary",
        })
    }
}
