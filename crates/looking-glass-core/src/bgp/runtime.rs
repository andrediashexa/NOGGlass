//! The socket layer: opening the sessions and feeding their events to the RIB.
//!
//! This is the thin, un-unit-testable half of the feature — it talks to a real
//! neighbour, so its proof is a live session against the lab, not a test. All
//! the routing logic it drives lives in [`session::apply_bgp_event`] and
//! [`rib`], which are tested on their own.
//!
//! Read-only toward the network: [`EchoCapabilitiesPolicy`] with no advertised
//! capabilities beyond what the peer offers, and NOGGlass never sends an UPDATE
//! — it opens the session, keeps it alive, and listens.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use netgauze_bgp_speaker::connection::TcpActiveConnect;
use netgauze_bgp_speaker::peer::{EchoCapabilitiesPolicy, PeerConfigBuilder, PeerProperties};
use netgauze_bgp_speaker::supervisor::PeersSupervisor;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::bgp::config::BgpSettings;
use crate::bgp::rib::LocalRib;
use crate::bgp::session::apply_bgp_event;

/// The RIB, shared between the session tasks that fill it and the API that reads
/// it.
pub type SharedRib = Arc<RwLock<LocalRib>>;

/// A running set of BGP sessions and the RIB they fill.
///
/// Holding this keeps the sessions alive: dropping it drops the supervisor,
/// which aborts every peer task. The server keeps it for the process lifetime
/// and hands [`Self::rib`] to the API.
pub struct BgpRuntime {
    // Owns the peer tasks; kept only to keep them running.
    _supervisor: PeersSupervisor<IpAddr, SocketAddr, TcpStream>,
    rib: SharedRib,
}

impl BgpRuntime {
    /// A shared handle to the RIB, for the API to read.
    pub fn rib(&self) -> SharedRib {
        self.rib.clone()
    }
}

const BGP_PORT: u16 = 179;

/// Starts the sessions described by `settings` and returns a handle that owns
/// them. MUST be called on a Tokio runtime; each peer runs on its own task.
///
/// Active peers only for now — NOGGlass dials the router. Passive peers (a
/// router that dials in, via a listener) are skipped here and arrive in a later
/// change.
pub fn spawn(settings: &BgpSettings) -> BgpRuntime {
    let rib: SharedRib = Arc::new(RwLock::new(LocalRib::new()));

    // Validated before we get here (config::BgpSettings::validate), but stay
    // fail-safe rather than panic if called out of order: no ids, no sessions.
    let (Some(my_asn), Some(my_bgp_id)) = (settings.local_as, settings.router_id) else {
        warn!("bgp: enabled without local_as/router_id; no sessions started");
        return BgpRuntime {
            _supervisor: PeersSupervisor::new(0, std::net::Ipv4Addr::UNSPECIFIED),
            rib,
        };
    };

    let mut supervisor = PeersSupervisor::new(my_asn, my_bgp_id);

    for peer in &settings.peers {
        if peer.passive {
            info!(peer = %peer.id, "bgp: passive peer skipped (listener is a later slice)");
            continue;
        }
        let peer_addr = SocketAddr::new(peer.host, BGP_PORT);
        let config = PeerConfigBuilder::new().build();
        let policy = EchoCapabilitiesPolicy::new(
            my_asn,
            true,
            my_bgp_id,
            config.hold_timer_duration_large_value().as_secs() as u16,
            vec![],
            vec![],
        );
        // allow_dynamic_as = false: we peer only with the AS we configured.
        let properties = PeerProperties::new(my_asn, peer.remote_as, my_bgp_id, peer_addr, false);

        match supervisor.create_peer(peer.host, properties, config, TcpActiveConnect, policy) {
            Ok((mut received_rx, handle)) => {
                if let Err(err) = handle.start() {
                    warn!(peer = %peer.id, %err, "bgp: could not start peer");
                    continue;
                }
                info!(peer = %peer.id, host = %peer.host, remote_as = peer.remote_as, "bgp: session starting");
                let rib = rib.clone();
                let peer_ip = peer.host;
                tokio::spawn(async move {
                    while let Some(result) = received_rx.recv().await {
                        match result {
                            Ok((_state, event)) => {
                                if let netgauze_bgp_speaker::events::BgpEvent::UpdateMsgErr(err) =
                                    &event
                                {
                                    warn!(peer = %peer_ip, ?err, "bgp: peer sent an UPDATE we could not parse; the session will reset");
                                }
                                let mut guard = rib.write().await;
                                apply_bgp_event(&mut guard, peer_ip, &event);
                            }
                            Err(err) => {
                                // The peer task reported a terminal error; its
                                // routes are no longer trustworthy.
                                warn!(peer = %peer_ip, %err, "bgp: peer error, dropping its routes");
                                rib.write().await.remove_peer(peer_ip);
                            }
                        }
                    }
                });
            }
            Err(err) => warn!(peer = %peer.id, ?err, "bgp: could not create peer"),
        }
    }

    BgpRuntime {
        _supervisor: supervisor,
        rib,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The socket runtime's real proof is a live session; here we only confirm
    /// it wires up on a Tokio runtime without a network — an all-passive config
    /// starts no active session, so the RIB comes up empty and shareable.
    #[tokio::test]
    async fn spawn_with_no_active_peers_starts_an_empty_rib() {
        let settings: BgpSettings = toml::from_str(
            r#"
            enabled = true
            local_as = 64500
            router_id = "192.0.2.1"
            listen = ["[::]:179"]
            [[peer]]
            id = "dials-in"
            host = "192.0.2.2"
            remote_as = 64496
            passive = true
            "#,
        )
        .unwrap();
        settings.validate().unwrap();

        let runtime = spawn(&settings);
        assert!(runtime.rib().read().await.is_empty());
    }
}
