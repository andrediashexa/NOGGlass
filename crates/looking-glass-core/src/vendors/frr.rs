use crate::driver::{
    BgpRouteResult, BgpSummaryResult, DriverError, PingResult, TracerouteResult, VendorDriver,
};

pub struct FrrDriver;

impl VendorDriver for FrrDriver {
    fn vendor_name(&self) -> &'static str {
        "frr"
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
    fn parses_frr_bgp_detail_blocks() {
        let raw = r#"BGP routing table entry for 198.51.100.0/24
Paths: (2 available, best #1, table default)
  Advertised to non-peer-group peers:
    192.0.2.2 192.0.2.3
  65100 65500
    192.0.2.254 from 192.0.2.254 (192.0.2.254)
      Origin IGP, metric 10, localpref 150, valid, external, best (First path received)
      Last update: Mon Sep 21 06:56:50 2026
  65200 65500
    192.0.2.253 from 192.0.2.253 (192.0.2.253)
      Origin IGP, metric 20, localpref 100, valid, external
      Last update: Mon Sep 21 06:56:40 2026
"#;
        let result = FrrDriver
            .parse_bgp_route(raw)
            .expect("FRR detailed BGP output should parse");

        assert_eq!(result.paths.len(), 2);
        let best = result.best().expect("best path is identified");
        assert_eq!(best.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(best.next_hop.unwrap().to_string(), "192.0.2.254");
        assert_eq!(best.as_path, vec![65100, 65500]);
        assert_eq!(best.med, Some(10));
        assert_eq!(best.local_pref, Some(150));
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
    fn parses_frr_bgp_tables() {
        let raw = r#"   Network          Next Hop            Metric LocPrf Weight Path
*> 198.51.100.0/24  192.0.2.254             10    150      0 65100 65500 i
*                   192.0.2.253             20    100      0 65200 65500 i
"#;
        let result = FrrDriver
            .parse_bgp_route(raw)
            .expect("FRR tabular BGP output should parse");

        assert_eq!(result.paths.len(), 2);
        let best = result.best().expect("best path should exist");
        assert_eq!(best.prefix.unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(best.as_path, vec![65100, 65500]);
        assert_eq!(best.local_pref, Some(150));
        assert_eq!(best.origin, Some(Origin::Igp));
    }

    #[test]
    fn parses_frr_bgp_summary() {
        let raw = r#"IPv4 Unicast Summary:
BGP router identifier 192.0.2.1, local AS number 65001 vrf-id 0
BGP table version 42
RIB entries 10, using 1920 bytes of memory
Peers 2, using 43 KiB of memory

Neighbor        V         AS   MsgRcvd   MsgSent   TblVer  InQ OutQ  Up/Down State/PfxRcd   PfxSnt
192.0.2.254     4      65100        15        15        0    0    0 00:04:12           15        5
192.0.2.253     4      65200         0         0        0    0    0    never         Idle        0
"#;
        let result = FrrDriver
            .parse_bgp_summary(raw)
            .expect("FRR BGP summary should parse");

        assert_eq!(result.router_id.as_deref(), Some("192.0.2.1"));
        assert_eq!(result.local_as, Some(65001));
        assert_eq!(result.peers.len(), 2);

        let up = &result.peers[0];
        assert_eq!(up.peer_ip, "192.0.2.254");
        assert_eq!(up.peer_as, 65100);
        assert_eq!(up.state, "Established");
        assert_eq!(up.uptime, "00:04:12");
        assert_eq!(up.prefixes_received, 15);

        let down = &result.peers[1];
        assert_eq!(down.peer_ip, "192.0.2.253");
        assert_eq!(down.peer_as, 65200);
        assert_eq!(down.state, "Idle");
        assert_eq!(down.prefixes_received, 0);
    }

    #[test]
    fn parses_frr_ping() {
        let raw = "\
PING 198.51.100.1 (198.51.100.1) 56(84) bytes of data.
64 bytes from 198.51.100.1: icmp_seq=1 ttl=64 time=0.231 ms

--- 198.51.100.1 ping statistics ---
5 packets transmitted, 5 received, 0% packet loss, time 4004ms
rtt min/avg/max/mdev = 0.198/0.211/0.231/0.015 ms
";
        let result = FrrDriver.parse_ping(raw).unwrap();
        assert_eq!(result.packets_sent, 5);
        assert_eq!(result.packets_received, 5);
        assert_eq!(result.packet_loss_percent, 0.0);
        assert_eq!(result.min_rtt_ms, Some(0.198));
        assert_eq!(result.avg_rtt_ms, Some(0.211));
        assert_eq!(result.max_rtt_ms, Some(0.231));
    }

    #[test]
    fn parses_frr_traceroute() {
        let raw = "\
traceroute to 198.51.100.1 (198.51.100.1), 30 hops max, 60 byte packets
 1  192.0.2.1 (192.0.2.1)  0.601 ms  0.589 ms  0.570 ms
 2  198.51.100.1 (198.51.100.1)  1.210 ms  1.190 ms  1.180 ms
";
        let result = FrrDriver.parse_traceroute(raw).unwrap();
        assert_eq!(result.hops.len(), 2);
        assert_eq!(result.hops[0].hop, 1);
        assert_eq!(result.hops[0].ip.as_deref(), Some("192.0.2.1"));
        assert_eq!(result.hops[1].hop, 2);
        assert_eq!(result.hops[1].ip.as_deref(), Some("198.51.100.1"));
    }

    #[test]
    fn empty_route_and_failed_ping_fail_closed() {
        let empty_bgp = "% Network not in table\n";
        let result = FrrDriver.parse_bgp_route(empty_bgp).unwrap();
        assert!(result.paths.is_empty());
        assert_eq!(result.raw_output, empty_bgp);

        assert!(FrrDriver.parse_ping("ping: unknown host\n").is_err());
    }
}
