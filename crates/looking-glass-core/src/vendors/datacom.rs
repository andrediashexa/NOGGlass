use crate::driver::{
    BgpRouteResult, BgpSummaryResult, DriverError, PingResult, TracerouteResult, VendorDriver,
};

pub struct DatacomDriver;

impl VendorDriver for DatacomDriver {
    fn vendor_name(&self) -> &'static str {
        "datacom_dmos"
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        crate::ping::parse(raw)
    }

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        // The shared reader: vendors differ in decoration, not in substance,
        // and a silent hop has to survive in every one of them.
        Ok(crate::traceroute::parse("", raw))
    }

    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        // DmOS prints the same table as IOS: status flags, network, next hop,
        // metric, local preference, weight, AS path, origin marker.
        Ok(crate::bgp_table::parse(raw))
    }

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        // Shared reader: the tables differ in headers, not in what a row means.
        Ok(crate::summary::parse(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Origin;

    /// DmOS prints the table IOS made standard, so it reads through the shared
    /// reader rather than through a second copy of the same logic.
    #[test]
    fn reads_the_shared_tabular_format() {
        // A raw string: `"\` strips the leading whitespace of the next line,
        // which would take the three spaces that line the header up with the
        // rows and leave a table no router prints.
        let raw = r#"   Network            Next Hop         Metric LocPrf Weight Path
*> 198.51.100.0/24    192.0.2.254          10    150      0 65100 65500 i
*                    192.0.2.253          20    100      0 65200 65500 i
"#;
        let result = DatacomDriver
            .parse_bgp_route(raw)
            .expect("DmOS output should parse");

        assert_eq!(result.paths.len(), 2);
        let best = result.best().expect("the > flag marks the best path");
        assert_eq!(best.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(best.as_path, vec![65100, 65500]);
        assert_eq!(best.local_pref, Some(150));
        assert_eq!(best.origin, Some(Origin::Igp));

        assert_eq!(
            result.paths[1].prefix.unwrap().to_string(),
            "198.51.100.0/24",
            "a continuation line keeps the prefix"
        );
    }

    #[test]
    fn reads_the_ping_summary() {
        let raw = "5 packets transmitted, 4 packets received, 20% packet loss\nround-trip min/avg/max = 1.0/2.0/3.0 ms\n";
        let result = DatacomDriver.parse_ping(raw).unwrap();
        assert_eq!(result.packets_received, 4);
        assert_eq!(result.packet_loss_percent, 20.0);
    }

    /// Neither query may fall back to invented data now that both are real.
    #[test]
    fn nothing_is_reported_that_the_output_did_not_contain() {
        let result = DatacomDriver
            .parse_bgp_route("% No matching routes found\n")
            .unwrap();
        assert!(result.paths.is_empty());
        assert!(DatacomDriver.parse_ping("ping: unknown host\n").is_err());
    }
}
