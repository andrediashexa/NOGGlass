//! The socket layer: listening for routers and feeding their BMP stream to the
//! RIB.
//!
//! Mirror of [`crate::bgp::runtime`] for the passive side. The routing logic it
//! drives lives in [`crate::bmp::mapping::apply_bmp_message`] and
//! [`crate::bgp::rib`], which are tested on their own; this half opens sockets,
//! so its proof is a real byte stream (see the tests, which drive a live
//! loopback session), not a mocked one.
//!
//! Read-only by the protocol: BMP is a monitoring feed. NOGGlass listens,
//! parses, and records — it never sends routing state back to a router.
//!
//! Fail closed toward untrusted input (RFC 7854 §3.2, BMP has no auth):
//! - a connection from an address outside the configured allow-list is
//!   refused before a byte is read;
//! - a frame that will not decode ends that session rather than being guessed.

use std::net::{IpAddr, SocketAddr};

use netgauze_bmp_pkt::codec::BmpCodec;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;
use tokio_util::codec::FramedRead;
use tracing::{info, warn};

use crate::bmp::config::{BmpRouterConfig, BmpSettings};
use crate::bmp::mapping::apply_bmp_message;
use crate::bmp::rib::BmpRibs;

/// Whether a connecting address may talk to the station, and under what key its
/// routes are held.
#[derive(Debug, PartialEq)]
enum Admission<'a> {
    /// A configured router, held under its id.
    Known(&'a str),
    /// No allow-list configured; held under the source address.
    Unlisted,
    /// An address outside a non-empty allow-list: refuse.
    Refused,
}

/// Decides whether `addr` may connect, given the configured routers.
///
/// An empty allow-list admits any address ([`Admission::Unlisted`]) — the
/// operator is trusting network ACLs, which the config documents. A non-empty
/// allow-list admits only its addresses and refuses the rest (fail closed).
fn admit(routers: &[BmpRouterConfig], addr: IpAddr) -> Admission<'_> {
    if routers.is_empty() {
        return Admission::Unlisted;
    }
    match routers.iter().find(|router| router.address == addr) {
        Some(router) => Admission::Known(&router.id),
        None => Admission::Refused,
    }
}

/// A running BMP station: the listeners accepting routers and the per-router
/// RIBs they fill.
///
/// Holding this keeps the listeners alive; dropping it aborts them. The server
/// keeps it for the process lifetime and hands [`Self::ribs`] to the API.
pub struct BmpStation {
    // Owns the accept-loop tasks; kept only to keep them running.
    _listeners: Vec<JoinHandle<()>>,
    // The addresses actually bound, for logging and tests (a `:0` request
    // resolves to a concrete port here).
    local_addrs: Vec<SocketAddr>,
    ribs: BmpRibs,
}

impl BmpStation {
    /// A shared handle to the per-router RIBs, for the API to read.
    pub fn ribs(&self) -> BmpRibs {
        self.ribs.clone()
    }

    /// The addresses the station is actually listening on.
    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.local_addrs
    }
}

/// Binds the configured listeners and returns a handle that owns them. MUST be
/// called on a Tokio runtime; each listener runs its accept loop on a task.
///
/// A listener that fails to bind is logged and skipped rather than aborting the
/// others — one unusable address must not take the station down.
pub async fn spawn(settings: &BmpSettings) -> BmpStation {
    let ribs = BmpRibs::new();

    if settings.routers.is_empty() {
        warn!(
            "bmp: no [[bmp.router]] allow-list configured; the station will accept any source that reaches a listener, relying on network ACLs"
        );
    }

    let mut listeners = Vec::new();
    let mut local_addrs = Vec::new();
    for addr in &settings.listen {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let bound = listener.local_addr().unwrap_or(*addr);
                info!(%bound, "bmp: station listening");
                local_addrs.push(bound);
                let ribs = ribs.clone();
                let routers = settings.routers.clone();
                listeners.push(tokio::spawn(accept_loop(listener, routers, ribs)));
            }
            Err(err) => warn!(%addr, %err, "bmp: could not bind listener"),
        }
    }

    BmpStation {
        _listeners: listeners,
        local_addrs,
        ribs,
    }
}

/// Accepts connections on one listener forever, admitting or refusing each by
/// the allow-list and handing the admitted ones their own reader task, keyed by
/// the router they belong to.
async fn accept_loop(listener: TcpListener, routers: Vec<BmpRouterConfig>, ribs: BmpRibs) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let key = match admit(&routers, peer.ip()) {
                    Admission::Refused => {
                        warn!(%peer, "bmp: refusing a connection from an address not in the allow-list");
                        // Dropping `stream` closes it.
                        continue;
                    }
                    Admission::Known(id) => {
                        info!(%peer, router = %id, "bmp: router connected");
                        id.to_string()
                    }
                    Admission::Unlisted => {
                        info!(%peer, "bmp: source connected (no allow-list)");
                        peer.ip().to_string()
                    }
                };
                tokio::spawn(handle_connection(stream, peer, key, ribs.clone()));
            }
            Err(err) => {
                warn!(%err, "bmp: accept failed");
                // Back off briefly so a persistent accept error is not a hot loop.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

/// Reads one router's BMP stream to end of session into that router's RIB, keyed
/// by `key`. A decode error ends the session (fail closed).
///
/// The RIB starts fresh for a reconnecting router and is dropped when its last
/// session closes, so a disconnected router stops answering rather than serving
/// a stale table (see [`BmpRibs`]).
async fn handle_connection(stream: TcpStream, peer: SocketAddr, key: String, ribs: BmpRibs) {
    let session = ribs.on_connect(&key).await;
    let mut frames = FramedRead::new(stream, BmpCodec::default());
    let mut warned_unmapped = false;
    while let Some(frame) = frames.next().await {
        match frame {
            Ok(message) => {
                // Fail closed rather than silently: if the peer speaks a BMP
                // version we do not map yet, its routes will not appear, so say
                // so once instead of leaving the RIB mysteriously empty.
                if !warned_unmapped {
                    if let Some(version) = crate::bmp::mapping::unmapped_version(&message) {
                        warn!(%peer, %version, "bmp: this peer's BMP version is not mapped yet; its routes will not appear");
                        warned_unmapped = true;
                    }
                }
                apply_bmp_message(&mut *session.rib.write().await, &message);
                crate::bmp::mapping::record_peer_event(&mut *session.peers.write().await, &message);
            }
            Err(err) => {
                warn!(%peer, ?err, "bmp: could not decode a message; closing the session");
                break;
            }
        }
    }
    ribs.on_disconnect(&key).await;
    info!(%peer, "bmp: session closed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipnet::Ipv4Net;
    use netgauze_bgp_pkt::nlri::{Ipv4Unicast, Ipv4UnicastAddress};
    use netgauze_bgp_pkt::path_attribute::{
        As4PathSegment, AsPath, AsPathSegmentType, NextHop, PathAttribute, PathAttributeValue,
    };
    use netgauze_bgp_pkt::update::BgpUpdateMessage;
    use netgauze_bgp_pkt::BgpMessage;
    use netgauze_bmp_pkt::v3::{BmpMessageValue, RouteMonitoringMessage};
    use netgauze_bmp_pkt::{BmpMessage, BmpPeerType, PeerHeader};
    use std::net::Ipv4Addr;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio_util::codec::Encoder;

    fn router(id: &str, address: &str) -> BmpRouterConfig {
        BmpRouterConfig {
            id: id.to_string(),
            address: address.parse().unwrap(),
            description: None,
        }
    }

    #[test]
    fn an_empty_allow_list_admits_any_address() {
        assert_eq!(
            admit(&[], "192.0.2.9".parse().unwrap()),
            Admission::Unlisted
        );
    }

    #[test]
    fn a_listed_address_is_admitted_and_named() {
        let routers = vec![router("edge01", "192.0.2.2")];
        assert_eq!(
            admit(&routers, "192.0.2.2".parse().unwrap()),
            Admission::Known("edge01")
        );
    }

    #[test]
    fn an_unlisted_address_is_refused_when_a_list_exists() {
        let routers = vec![router("edge01", "192.0.2.2")];
        assert_eq!(
            admit(&routers, "192.0.2.3".parse().unwrap()),
            Admission::Refused
        );
    }

    /// Builds a BMP Route Monitoring message advertising `prefix` from a
    /// monitored peer, encoded to the wire exactly as a router would send it.
    fn route_monitoring_bytes(peer: &str, prefix: &str) -> Vec<u8> {
        let net: Ipv4Net = prefix.parse().unwrap();
        let update = BgpUpdateMessage::new(
            vec![],
            vec![
                PathAttribute::from(
                    false,
                    true,
                    false,
                    false,
                    PathAttributeValue::AsPath(AsPath::As4PathSegments(
                        vec![As4PathSegment::new(
                            AsPathSegmentType::AsSequence,
                            vec![65001],
                        )]
                        .into(),
                    )),
                )
                .unwrap(),
                PathAttribute::from(
                    false,
                    true,
                    false,
                    false,
                    PathAttributeValue::NextHop(NextHop::new(Ipv4Addr::new(192, 0, 2, 254))),
                )
                .unwrap(),
            ],
            vec![Ipv4UnicastAddress::new_no_path_id(
                Ipv4Unicast::from_net(net).unwrap(),
            )],
        );
        let header = PeerHeader::new(
            BmpPeerType::GlobalInstancePeer {
                ipv6: false,
                post_policy: false,
                asn2: false,
                adj_rib_out: false,
            },
            None,
            Some(peer.parse().unwrap()),
            64496,
            Ipv4Addr::new(192, 0, 2, 254),
            None,
        );
        let rm = RouteMonitoringMessage::build(header, BgpMessage::Update(update)).unwrap();
        let message = BmpMessage::V3(BmpMessageValue::RouteMonitoring(rm));

        let mut buf = bytes::BytesMut::new();
        BmpCodec::default().encode(message, &mut buf).unwrap();
        buf.to_vec()
    }

    #[tokio::test]
    async fn a_connected_router_fills_the_rib() {
        let settings: BmpSettings = toml::from_str(
            r#"
            enabled = true
            listen = ["127.0.0.1:0"]
            [[router]]
            id = "loopback"
            address = "127.0.0.1"
            "#,
        )
        .unwrap();
        settings.validate().unwrap();

        let station = spawn(&settings).await;
        let addr = station.local_addrs()[0];

        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(&route_monitoring_bytes("192.0.2.1", "203.0.113.0/24"))
            .await
            .unwrap();
        client.flush().await.unwrap();

        // The reader task runs concurrently; poll the router's RIB until the
        // route lands. The connection is keyed by the configured router id.
        let prefix = "203.0.113.0/24".parse().unwrap();
        let mut filled = false;
        for _ in 0..50 {
            if let Some(rib) = station.ribs().get("loopback").await {
                if !rib.read().await.paths_for(&prefix).is_empty() {
                    filled = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(filled, "the monitored route should reach the router's RIB");

        let rib = station.ribs().get("loopback").await.unwrap();
        let guard = rib.read().await;
        let paths = guard.paths_for(&prefix);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].peer, Some("192.0.2.1".parse().unwrap()));
    }

    #[tokio::test]
    async fn a_refused_source_leaves_the_rib_empty() {
        let settings: BmpSettings = toml::from_str(
            r#"
            enabled = true
            listen = ["127.0.0.1:0"]
            [[router]]
            id = "elsewhere"
            address = "192.0.2.2"
            "#,
        )
        .unwrap();
        settings.validate().unwrap();

        let station = spawn(&settings).await;
        let addr = station.local_addrs()[0];

        // Connecting from 127.0.0.1, which is not the allow-listed 192.0.2.2.
        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = client
            .write_all(&route_monitoring_bytes("192.0.2.1", "203.0.113.0/24"))
            .await;

        tokio::time::sleep(Duration::from_millis(150)).await;
        // A refused source is never keyed, so no RIB exists for it.
        assert!(station.ribs().live_keys().await.is_empty());
    }
}
