//! Configuration for the BGP session NOGGlass holds itself (issue #171).
//!
//! An operator points their own router at NOGGlass — or lets NOGGlass dial the
//! router — and NOGGlass keeps the received routes in a local RIB, answering
//! the looking glass from that live table instead of a login per query. The
//! engine is NetGauze, embedded in the binary ([ADR-0016]).
//!
//! This module is the configuration surface only: the `[bgp]` section of the
//! inventory. The session runtime (the NetGauze supervisor, the per-peer event
//! loop) and the mapping from received UPDATEs into the normalised path model
//! land in later changes and consume what is validated here.
//!
//! Three postures are fixed by the design and are not configurable:
//!
//! - **Read-only.** NOGGlass never advertises a prefix; the export policy is
//!   reject-all. There is deliberately no "announce" field to set.
//! - **Disabled by default.** `enabled` is `false` unless the operator turns it
//!   on, so a fresh install opens no BGP socket.
//! - **Fail closed.** An `enabled` section that is missing what a session needs
//!   is a startup error, not a half-configured speaker.
//!
//! [ADR-0016]: ../../docs/adr/0016-one-rust-engine-for-bgp-session-and-bmp.md

use serde::Deserialize;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// The `[bgp]` section: whether NOGGlass runs its own session, and with whom.
///
/// The derived `Default` is the disabled, empty section — a fresh install opens
/// no BGP socket.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BgpSettings {
    /// Off by default. While off, no socket is opened and the rest of this
    /// section is ignored except that it MUST still parse.
    #[serde(default)]
    pub enabled: bool,
    /// The AS NOGGlass speaks as. Required when `enabled`.
    #[serde(default)]
    pub local_as: Option<u32>,
    /// The BGP identifier NOGGlass presents. Required when `enabled`. An IPv4
    /// address by protocol, even on an IPv6-only session.
    #[serde(default)]
    pub router_id: Option<Ipv4Addr>,
    /// Addresses NOGGlass listens on for peers that dial in (passive peers).
    /// May be empty when every peer is active (NOGGlass dials the router).
    #[serde(default)]
    pub listen: Vec<SocketAddr>,
    /// The routers NOGGlass peers with.
    #[serde(default, rename = "peer")]
    pub peers: Vec<BgpPeerConfig>,
}

/// One router NOGGlass peers with.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BgpPeerConfig {
    /// Stable identifier, used in the API and logs.
    pub id: String,
    /// The peer's address.
    pub host: IpAddr,
    /// The peer's AS. Zero is not a real AS (RFC 7607) and is rejected.
    pub remote_as: u32,
    /// When true, NOGGlass waits for the router to open the session rather than
    /// dialing it. A passive peer needs a `listen` address to arrive on.
    #[serde(default)]
    pub passive: bool,
    /// Optional human note shown in the interface.
    #[serde(default)]
    pub description: Option<String>,
}

impl BgpSettings {
    /// Checks the section makes sense before the process starts.
    ///
    /// Returns `Ok` immediately when disabled — a turned-off section is never a
    /// reason to refuse to boot. When enabled, everything a session needs MUST
    /// be present, or the message names what is missing (fail closed).
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        if self.local_as.is_none() {
            return Err("bgp.local_as is required when bgp.enabled is true".to_string());
        }
        if self.router_id.is_none() {
            return Err("bgp.router_id is required when bgp.enabled is true".to_string());
        }
        if self.peers.is_empty() {
            return Err("bgp.enabled is true but no [[bgp.peer]] is configured".to_string());
        }

        let mut ids = BTreeSet::new();
        let mut hosts = BTreeSet::new();
        for peer in &self.peers {
            if peer.id.trim().is_empty() {
                return Err("a [[bgp.peer]] has an empty id".to_string());
            }
            if peer.remote_as == 0 {
                return Err(format!(
                    "bgp.peer '{}' has remote_as 0, which is not a real AS",
                    peer.id
                ));
            }
            if !ids.insert(peer.id.clone()) {
                return Err(format!("bgp.peer id '{}' is used more than once", peer.id));
            }
            if !hosts.insert(peer.host) {
                return Err(format!(
                    "bgp.peer host '{}' is configured more than once",
                    peer.host
                ));
            }
            if peer.passive && self.listen.is_empty() {
                return Err(format!(
                    "bgp.peer '{}' is passive but bgp.listen is empty, so it can never connect",
                    peer.id
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> BgpSettings {
        toml::from_str(source).expect("valid toml")
    }

    #[test]
    fn the_default_is_disabled_and_empty() {
        let bgp = BgpSettings::default();
        assert!(!bgp.enabled);
        assert!(bgp.peers.is_empty());
        // A disabled section never blocks startup, even with nothing else set.
        assert!(bgp.validate().is_ok());
    }

    #[test]
    fn a_disabled_section_parses_and_validates_with_no_other_fields() {
        let bgp = parse("enabled = false");
        assert!(!bgp.enabled);
        assert!(bgp.validate().is_ok());
    }

    #[test]
    fn a_well_formed_enabled_section_validates() {
        let bgp = parse(
            r#"
            enabled = true
            local_as = 64500
            router_id = "192.0.2.1"

            [[peer]]
            id = "edge01"
            host = "192.0.2.2"
            remote_as = 64496
            "#,
        );
        assert!(bgp.enabled);
        assert_eq!(bgp.local_as, Some(64500));
        assert_eq!(bgp.peers.len(), 1);
        assert_eq!(bgp.peers[0].remote_as, 64496);
        assert!(!bgp.peers[0].passive);
        bgp.validate().expect("should be valid");
    }

    #[test]
    fn enabled_without_local_as_is_refused() {
        let bgp = parse(
            r#"
            enabled = true
            router_id = "192.0.2.1"
            [[peer]]
            id = "a"
            host = "192.0.2.2"
            remote_as = 64496
            "#,
        );
        assert!(bgp.validate().unwrap_err().contains("local_as"));
    }

    #[test]
    fn enabled_without_router_id_is_refused() {
        let bgp = parse(
            r#"
            enabled = true
            local_as = 64500
            [[peer]]
            id = "a"
            host = "192.0.2.2"
            remote_as = 64496
            "#,
        );
        assert!(bgp.validate().unwrap_err().contains("router_id"));
    }

    #[test]
    fn enabled_with_no_peers_is_refused() {
        let bgp = parse(
            r#"
            enabled = true
            local_as = 64500
            router_id = "192.0.2.1"
            "#,
        );
        assert!(bgp.validate().unwrap_err().contains("no [[bgp.peer]]"));
    }

    #[test]
    fn a_peer_with_as_zero_is_refused() {
        let bgp = parse(
            r#"
            enabled = true
            local_as = 64500
            router_id = "192.0.2.1"
            [[peer]]
            id = "a"
            host = "192.0.2.2"
            remote_as = 0
            "#,
        );
        assert!(bgp.validate().unwrap_err().contains("not a real AS"));
    }

    #[test]
    fn duplicate_peer_hosts_are_refused() {
        let bgp = parse(
            r#"
            enabled = true
            local_as = 64500
            router_id = "192.0.2.1"
            [[peer]]
            id = "a"
            host = "192.0.2.2"
            remote_as = 64496
            [[peer]]
            id = "b"
            host = "192.0.2.2"
            remote_as = 64497
            "#,
        );
        assert!(bgp.validate().unwrap_err().contains("more than once"));
    }

    #[test]
    fn a_passive_peer_without_a_listen_address_is_refused() {
        let bgp = parse(
            r#"
            enabled = true
            local_as = 64500
            router_id = "192.0.2.1"
            [[peer]]
            id = "a"
            host = "192.0.2.2"
            remote_as = 64496
            passive = true
            "#,
        );
        assert!(bgp.validate().unwrap_err().contains("passive"));
    }

    #[test]
    fn a_passive_peer_with_a_listen_address_is_fine() {
        let bgp = parse(
            r#"
            enabled = true
            local_as = 64500
            router_id = "192.0.2.1"
            listen = ["[::]:179"]
            [[peer]]
            id = "a"
            host = "2001:db8::2"
            remote_as = 64496
            passive = true
            "#,
        );
        bgp.validate().expect("should be valid");
        assert_eq!(bgp.listen.len(), 1);
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        // deny_unknown_fields guards against a typo silently doing nothing —
        // a misspelled `enabled` MUST NOT leave a session quietly off.
        let err = toml::from_str::<BgpSettings>("enable = true").unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}
