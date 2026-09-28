//! Per-router RIBs for the BMP station.
//!
//! Each monitored router gets its own [`LocalRib`], so one router's view is
//! isolated from another's and a router that goes away takes only its own routes
//! with it. A "router key" is the configured `[[bmp.router]]` id when the source
//! is on the allow-list, or the source address when the station accepts any
//! source (empty allow-list) — either way, one key, one RIB.
//!
//! Liveness is reference-counted so the common flap is handled cleanly:
//! - the first connection for a key installs a **fresh** RIB, so a reconnecting
//!   router re-synchronises from empty rather than merging onto stale routes;
//! - a second concurrent connection for the same key shares that RIB (a router
//!   briefly holding two sessions, which really happens, must not wipe itself);
//! - when the **last** connection for a key closes, its RIB is dropped, so a
//!   disconnected router stops answering rather than serving a stale table.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::bgp::rib::LocalRib;
use crate::bgp::runtime::SharedRib;

/// One monitored peer of a router, as BMP reports it: its AS (from the per-peer
/// header) and whether it is currently up (Peer Up seen, no later Peer Down).
#[derive(Debug, Clone)]
pub struct BmpPeer {
    pub asn: u32,
    pub up: bool,
}

/// A router's monitored peers, keyed by address, shared between the session task
/// that fills it and the API that reads it for the neighbour summary.
pub type SharedPeers = Arc<RwLock<BTreeMap<IpAddr, BmpPeer>>>;

/// The state one BMP session fills: the router's RIB and its peer table.
#[derive(Clone)]
pub struct BmpSession {
    pub rib: SharedRib,
    pub peers: SharedPeers,
}

/// One router's RIB, its monitored peers, and how many BMP sessions currently
/// feed it.
struct Entry {
    rib: SharedRib,
    peers: SharedPeers,
    live: usize,
}

/// The BMP station's per-router RIBs, shared between the session tasks that fill
/// them and the API that reads them.
#[derive(Clone, Default)]
pub struct BmpRibs {
    inner: Arc<RwLock<BTreeMap<String, Entry>>>,
}

impl BmpRibs {
    /// An empty set of RIBs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new BMP session for `key` and returns the state it should
    /// fill (the RIB and the peer table).
    ///
    /// The first session for a key gets fresh, empty state (clean re-sync); a
    /// concurrent second session shares the existing one.
    pub async fn on_connect(&self, key: &str) -> BmpSession {
        let mut guard = self.inner.write().await;
        let entry = guard.entry(key.to_string()).or_insert_with(|| Entry {
            rib: Arc::new(RwLock::new(LocalRib::new())),
            peers: Arc::new(RwLock::new(BTreeMap::new())),
            live: 0,
        });
        if entry.live == 0 {
            // First (or first after a full disconnect): start from empty so a
            // reconnecting router does not merge onto a stale table.
            entry.rib = Arc::new(RwLock::new(LocalRib::new()));
            entry.peers = Arc::new(RwLock::new(BTreeMap::new()));
        }
        entry.live += 1;
        BmpSession {
            rib: entry.rib.clone(),
            peers: entry.peers.clone(),
        }
    }

    /// Records that a BMP session for `key` has ended. When the last one ends,
    /// the router's RIB is dropped so it no longer answers with a stale table.
    pub async fn on_disconnect(&self, key: &str) {
        let mut guard = self.inner.write().await;
        if let Some(entry) = guard.get_mut(key) {
            entry.live = entry.live.saturating_sub(1);
            if entry.live == 0 {
                guard.remove(key);
            }
        }
    }

    /// The current RIB for `key`, if a session is feeding it.
    pub async fn get(&self, key: &str) -> Option<SharedRib> {
        self.inner
            .read()
            .await
            .get(key)
            .map(|entry| entry.rib.clone())
    }

    /// The current peer table for `key`, if a session is feeding it.
    pub async fn get_peers(&self, key: &str) -> Option<SharedPeers> {
        self.inner
            .read()
            .await
            .get(key)
            .map(|entry| entry.peers.clone())
    }

    /// The keys that currently have a live RIB, sorted.
    pub async fn live_keys(&self) -> Vec<String> {
        self.inner.read().await.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::BgpPath;

    fn a_path(prefix: &str, peer: &str) -> BgpPath {
        BgpPath {
            prefix: Some(prefix.parse().unwrap()),
            peer: Some(peer.parse().unwrap()),
            ..Default::default()
        }
    }

    async fn install(rib: &SharedRib, prefix: &str, peer: &str) {
        rib.write()
            .await
            .apply_update(peer.parse().unwrap(), vec![a_path(prefix, peer)], &[]);
    }

    #[tokio::test]
    async fn each_router_keeps_its_own_routes() {
        let ribs = BmpRibs::new();
        let a = ribs.on_connect("edge-a").await;
        let b = ribs.on_connect("edge-b").await;
        install(&a.rib, "203.0.113.0/24", "192.0.2.1").await;
        install(&b.rib, "198.51.100.0/24", "192.0.2.2").await;

        let a = ribs.get("edge-a").await.unwrap();
        let b = ribs.get("edge-b").await.unwrap();
        assert_eq!(a.read().await.prefix_count(), 1);
        assert_eq!(b.read().await.prefix_count(), 1);
        assert!(a
            .read()
            .await
            .paths_for(&"198.51.100.0/24".parse().unwrap())
            .is_empty());
    }

    #[tokio::test]
    async fn the_last_disconnect_drops_the_routers_rib() {
        let ribs = BmpRibs::new();
        let rib = ribs.on_connect("edge-a").await;
        install(&rib.rib, "203.0.113.0/24", "192.0.2.1").await;
        assert!(ribs.get("edge-a").await.is_some());

        ribs.on_disconnect("edge-a").await;
        assert!(ribs.get("edge-a").await.is_none(), "no stale table remains");
        assert_eq!(ribs.live_keys().await, Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_reconnect_starts_from_an_empty_table() {
        let ribs = BmpRibs::new();
        let first = ribs.on_connect("edge-a").await;
        install(&first.rib, "203.0.113.0/24", "192.0.2.1").await;
        ribs.on_disconnect("edge-a").await;

        let second = ribs.on_connect("edge-a").await;
        assert_eq!(
            second.rib.read().await.prefix_count(),
            0,
            "the reconnecting router re-syncs from empty"
        );
    }

    #[tokio::test]
    async fn a_concurrent_second_session_shares_the_rib_and_survives_one_close() {
        let ribs = BmpRibs::new();
        let first = ribs.on_connect("edge-a").await;
        install(&first.rib, "203.0.113.0/24", "192.0.2.1").await;
        // A second session arrives before the first closes; it must not wipe the
        // routes the first already installed.
        let second = ribs.on_connect("edge-a").await;
        assert_eq!(second.rib.read().await.prefix_count(), 1);

        // One of the two closes; the RIB stays because a session is still live.
        ribs.on_disconnect("edge-a").await;
        assert!(ribs.get("edge-a").await.is_some());
        // The last one closes; now it is dropped.
        ribs.on_disconnect("edge-a").await;
        assert!(ribs.get("edge-a").await.is_none());
    }
}
