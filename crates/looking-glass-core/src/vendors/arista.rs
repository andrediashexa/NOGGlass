use crate::driver::{
    BgpRouteResult, BgpSummaryResult, DriverError, PingResult, TracerouteResult, VendorDriver,
};

pub struct AristaDriver;

impl VendorDriver for AristaDriver {
    fn vendor_name(&self) -> &'static str {
        "arista_eos"
    }

    fn parse_ping(&self, raw: &str) -> Result<PingResult, DriverError> {
        crate::ping::parse(raw)
    }

    fn parse_traceroute(&self, raw: &str) -> Result<TracerouteResult, DriverError> {
        Ok(crate::traceroute::parse("", raw))
    }

    fn parse_bgp_route(&self, raw: &str) -> Result<BgpRouteResult, DriverError> {
        if raw.contains("BGP routing table entry for") {
            return Ok(crate::vendors::cisco::parse_detail_blocks(raw));
        }

        Ok(crate::bgp_table::parse(raw))
    }

    fn parse_bgp_summary(&self, raw: &str) -> Result<BgpSummaryResult, DriverError> {
        Ok(crate::summary::parse(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Origin;

    #[test]
    fn parses_arista_bgp_detail_blocks() {
        let raw = r#"BGP routing table information for VRF default
Router identifier 192.0.2.1, local AS number 65001
BGP routing table entry for 198.51.100.0/24
 Paths: 2 available
  65100 65500
    192.0.2.254 from 192.0.2.254 (192.0.2.254)
      Origin IGP, metric 10, localpref 150, weight 0, valid, external, best
  65200 65500
    192.0.2.253 from 192.0.2.253 (192.0.2.253)
      Origin IGP, metric 20, localpref 100, weight 0, valid, external
"#;
        let result = AristaDriver
            .parse_bgp_route(raw)
            .expect("Arista detailed BGP output should parse");

        assert_eq!(result.paths.len(), 2);
        let best = result.best().expect("best path is identified");
        assert_eq!(best.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(best.next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(best.as_path, vec![65100, 65500]);
        assert_eq!(best.med, Some(10));
        assert_eq!(best.local_pref, Some(150));
        assert_eq!(best.weight, Some(0));
        assert_eq!(best.origin, Some(Origin::Igp));
        assert!(best.is_best);

        let alt = &result.paths[1];
        assert_eq!(alt.next_hop.unwrap().to_string(), "192.0.2.253");
        assert_eq!(alt.as_path, vec![65200, 65500]);
        assert_eq!(alt.med, Some(20));
        assert_eq!(alt.local_pref, Some(100));
        assert!(!alt.is_best);
    }

    #[test]
    fn parses_arista_bgp_tables() {
        let raw = r#"   Network            Next Hop         Metric LocPrf Weight Path
*> 198.51.100.0/24    192.0.2.254          10    150      0 65100 65500 i
*                     192.0.2.253          20    100      0 65200 65500 i
"#;
        let result = AristaDriver
            .parse_bgp_route(raw)
            .expect("Arista tabular BGP output should parse");

        assert_eq!(result.paths.len(), 2);
        let best = result.best().expect("best path should exist");
        assert_eq!(best.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(best.as_path, vec![65100, 65500]);
        assert_eq!(best.local_pref, Some(150));
        assert_eq!(best.origin, Some(Origin::Igp));
    }

    #[test]
    fn parses_arista_bgp_summary() {
        let raw = r#"BGP summary information for VRF default
Router identifier 192.0.2.1, local AS number 65001
Neighbor Status Codes: m - Under maintenance
  Neighbor         V  AS           MsgRcvd   MsgSent  InQ OutQ  Up/Down State   PfxRcd PfxAcc
  192.0.2.254      4  65100             10        10    0    0 00:05:23 Estab       12     10
  192.0.2.253      4  65200              0         0    0    0 00:00:10 Idle
"#;
        let result = AristaDriver
            .parse_bgp_summary(raw)
            .expect("Arista BGP summary should parse");

        assert_eq!(result.router_id.as_deref(), Some("192.0.2.1"));
        assert_eq!(result.local_as, Some(65001));
        assert_eq!(result.peers.len(), 2);

        let up = &result.peers[0];
        assert_eq!(up.peer_ip, "192.0.2.254");
        assert_eq!(up.peer_as, 65100);
        assert_eq!(up.state, "Established");
        assert_eq!(up.uptime, "00:05:23");
        assert_eq!(up.prefixes_received, 12);
        assert_eq!(up.prefixes_accepted, Some(10));

        let down = &result.peers[1];
        assert_eq!(down.peer_ip, "192.0.2.253");
        assert_eq!(down.peer_as, 65200);
        assert_eq!(down.state, "Idle");
        assert_eq!(down.prefixes_received, 0);
        assert_eq!(down.prefixes_accepted, None);
    }

    #[test]
    fn parses_arista_ping() {
        let raw = "\
PING 198.51.100.1 (198.51.100.1) 72(100) bytes of data.
80 bytes from 198.51.100.1: icmp_seq=1 ttl=64 time=0.123 ms

--- 198.51.100.1 ping statistics ---
5 packets transmitted, 5 received, 0% packet loss, time 4004ms
rtt min/avg/max/mdev = 0.115/0.120/0.125/0.005 ms
";
        let result = AristaDriver.parse_ping(raw).unwrap();
        assert_eq!(result.packets_sent, 5);
        assert_eq!(result.packets_received, 5);
        assert_eq!(result.packet_loss_percent, 0.0);
        assert_eq!(result.min_rtt_ms, Some(0.115));
        assert_eq!(result.avg_rtt_ms, Some(0.120));
        assert_eq!(result.max_rtt_ms, Some(0.125));
    }

    #[test]
    fn parses_arista_traceroute() {
        let raw = "\
traceroute to 198.51.100.1 (198.51.100.1), 30 hops max, 60 byte packets
 1  192.0.2.1 (192.0.2.1)  0.512 ms  0.480 ms  0.450 ms
 2  198.51.100.1 (198.51.100.1)  1.120 ms  1.105 ms  1.090 ms
";
        let result = AristaDriver.parse_traceroute(raw).unwrap();
        assert_eq!(result.hops.len(), 2);
        assert_eq!(result.hops[0].hop, 1);
        assert_eq!(result.hops[0].ip.as_deref(), Some("192.0.2.1"));
        assert_eq!(result.hops[1].hop, 2);
        assert_eq!(result.hops[1].ip.as_deref(), Some("198.51.100.1"));
    }

    #[test]
    fn empty_route_and_failed_ping_fail_closed() {
        let empty_bgp = "% Network not in table\n";
        let result = AristaDriver.parse_bgp_route(empty_bgp).unwrap();
        assert!(result.paths.is_empty());
        assert_eq!(result.raw_output, empty_bgp);

        assert!(AristaDriver.parse_ping("ping: unknown host\n").is_err());
    }
}
