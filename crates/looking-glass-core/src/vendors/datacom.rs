use crate::driver::{
    BgpRouteResult, BgpSummaryResult, DriverError, PingResult, TracerouteResult, VendorDriver,
};

pub struct DatacomDriver;

impl VendorDriver for DatacomDriver {
    fn vendor_name(&self) -> &'static str {
        "datacom_dmos"
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

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
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
