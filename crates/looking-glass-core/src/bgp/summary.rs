//! The neighbour summary for NOGGlass's own BGP session (#186).
//!
//! Vendor routers answer `bgp_summary` with a neighbour table; this assembles
//! the same [`BgpSummaryResult`] for the local session from three sources that
//! each know part of it: the configuration (which peers, their remote AS), the
//! live FSM state the runtime tracks per peer, and the RIB (how many prefixes
//! each peer advertised). Kept pure — the runtime and the API hold the moving
//! parts, this only shapes them — so it is fully testable.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::bgp::config::BgpSettings;
use crate::bgp::rib::LocalRib;
use crate::driver::{BgpPeerSummary, BgpSummaryResult};

/// What the runtime knows about one peer's session right now.
#[derive(Debug, Clone)]
pub struct PeerStatus {
    /// The FSM state, as the engine names it (`Idle`, `Connect`, …,
    /// `Established`).
    pub state: String,
    /// When the session last reached `Established`, for the uptime column;
    /// `None` while it never has.
    pub established_since: Option<Instant>,
}

impl PeerStatus {
    /// A peer in `state` that is not (yet) established.
    pub fn new(state: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            established_since: None,
        }
    }
}

/// The per-peer session state, shared between the runtime that fills it and the
/// API that reads it.
pub type SessionStates = Arc<RwLock<BTreeMap<IpAddr, PeerStatus>>>;

/// Formats a session's age the compact way a neighbour table does.
fn format_uptime(duration: Duration) -> String {
    let secs = duration.as_secs();
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    let seconds = secs % 60;
    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{mins}m")
    } else if mins > 0 {
        format!("{mins}m{seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Builds the neighbour summary for the local session from its configuration,
/// the live per-peer state, and the RIB.
///
/// Every configured peer appears, so the table is stable whether or not a
/// session is up: a peer never seen shows `Idle` with no uptime and zero
/// prefixes. `now` is taken as an argument so the uptime is testable.
pub fn session_summary(
    settings: &BgpSettings,
    states: &BTreeMap<IpAddr, PeerStatus>,
    rib: &LocalRib,
    now: Instant,
) -> BgpSummaryResult {
    let peers = settings
        .peers
        .iter()
        .map(|peer| {
            let status = states.get(&peer.host);
            let state = status
                .map(|status| status.state.clone())
                .unwrap_or_else(|| "Idle".to_string());
            let uptime = status
                .and_then(|status| status.established_since)
                .map(|since| format_uptime(now.saturating_duration_since(since)))
                .unwrap_or_default();
            BgpPeerSummary {
                peer_ip: peer.host.to_string(),
                peer_as: peer.remote_as,
                state,
                uptime,
                prefixes_received: rib.prefix_count_for_peer(peer.host) as u32,
                prefixes_accepted: None,
            }
        })
        .collect();

    BgpSummaryResult {
        router_id: settings.router_id.map(|id| id.to_string()),
        local_as: settings.local_as,
        peers,
        raw_output: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::BgpPath;

    fn settings() -> BgpSettings {
        toml::from_str(
            r#"
            enabled = true
            local_as = 64500
            router_id = "192.0.2.1"
            [[peer]]
            id = "a"
            host = "192.0.2.10"
            remote_as = 65010
            [[peer]]
            id = "b"
            host = "192.0.2.20"
            remote_as = 65020
            "#,
        )
        .unwrap()
    }

    #[test]
    fn uptime_is_formatted_compactly() {
        assert_eq!(format_uptime(Duration::from_secs(5)), "5s");
        assert_eq!(format_uptime(Duration::from_secs(65)), "1m5s");
        assert_eq!(format_uptime(Duration::from_secs(3_661)), "1h1m");
        assert_eq!(format_uptime(Duration::from_secs(90_061)), "1d1h");
    }

    #[test]
    fn every_configured_peer_appears_even_when_never_seen() {
        let summary = session_summary(
            &settings(),
            &BTreeMap::new(),
            &LocalRib::new(),
            Instant::now(),
        );
        assert_eq!(summary.local_as, Some(64500));
        assert_eq!(summary.router_id.as_deref(), Some("192.0.2.1"));
        assert_eq!(summary.peers.len(), 2);
        // A peer never seen is Idle, zero prefixes, no uptime.
        let a = &summary.peers[0];
        assert_eq!(a.peer_ip, "192.0.2.10");
        assert_eq!(a.peer_as, 65010);
        assert_eq!(a.state, "Idle");
        assert_eq!(a.prefixes_received, 0);
        assert!(a.uptime.is_empty());
    }

    #[test]
    fn an_established_peer_shows_its_state_uptime_and_prefix_count() {
        let now = Instant::now();
        let mut states = BTreeMap::new();
        states.insert(
            "192.0.2.10".parse().unwrap(),
            PeerStatus {
                state: "Established".to_string(),
                established_since: Some(now - Duration::from_secs(3_661)),
            },
        );

        let mut rib = LocalRib::new();
        rib.apply_update(
            "192.0.2.10".parse().unwrap(),
            vec![
                BgpPath {
                    prefix: Some("203.0.113.0/24".parse().unwrap()),
                    peer: Some("192.0.2.10".parse().unwrap()),
                    ..Default::default()
                },
                BgpPath {
                    prefix: Some("198.51.100.0/24".parse().unwrap()),
                    peer: Some("192.0.2.10".parse().unwrap()),
                    ..Default::default()
                },
            ],
            &[],
        );

        let summary = session_summary(&settings(), &states, &rib, now);
        let a = &summary.peers[0];
        assert_eq!(a.state, "Established");
        assert_eq!(a.prefixes_received, 2);
        assert_eq!(a.uptime, "1h1m");
        // The second peer, unseen, stays Idle.
        assert_eq!(summary.peers[1].state, "Idle");
    }
}
