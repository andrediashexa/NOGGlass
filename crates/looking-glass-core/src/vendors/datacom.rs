use crate::driver::{
    BgpPath, BgpSummaryResult, DriverError, PingResult, QueryTarget, TracerouteResult, VendorDriver,
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

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        Ok(PingResult {
            packets_sent: 5,
            packets_received: 5,
            packet_loss_percent: 0.0,
            min_rtt_ms: None,
            avg_rtt_ms: None,
            max_rtt_ms: None,
            raw_output: raw.to_string(),
        })
    }

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        Ok(TracerouteResult {
            target: "".to_string(),
            hops: Vec::new(),
            raw_output: raw.to_string(),
        })
    }

    fn parse_bgp_route(&self, _raw: &str) -> Result<Vec<BgpPath>, DriverError> {
        Ok(Vec::new())
    }

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        Ok(BgpSummaryResult {
            router_id: None,
            local_as: None,
            peers: Vec::new(),
            raw_output: raw.to_string(),
        })
    }
}
