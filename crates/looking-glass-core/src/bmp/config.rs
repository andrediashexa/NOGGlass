//! Configuration for the BMP station NOGGlass runs (issue #172).
//!
//! An operator points their routers' BMP exporters at NOGGlass, which listens,
//! parses the stream (RFC 7854), and serves the monitored routes. The engine is
//! NetGauze, embedded in the binary ([ADR-0016]).
//!
//! This module is the configuration surface only: the `[bmp]` section of the
//! inventory. The station runtime (the listener, the per-session parser, the
//! per-peer RIBs) lands in later changes and consumes what is validated here.
//!
//! Fixed by the design, not configurable:
//!
//! - **Observe-only.** BMP never sends routing state to a router; there is no
//!   knob that turns the station into a speaker.
//! - **Disabled by default.** `enabled` is `false` unless the operator turns it
//!   on, so a fresh install opens no listener.
//! - **Untrusted input.** BMP carries no authentication (RFC 7854 §3.2), so the
//!   listener MUST be reachable only from configured routers; the parser MUST be
//!   defensive and fail closed. Enforced by the runtime, stated here.
//!
//! [ADR-0016]: ../../docs/adr/0016-one-rust-engine-for-bgp-session-and-bmp.md

use serde::Deserialize;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};

/// The `[bmp]` section: whether NOGGlass runs a BMP station, and where.
///
/// The `Default` is the disabled, empty station — a fresh install opens no
/// listener.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BmpSettings {
    /// Off by default. While off, no listener is opened and the rest of this
    /// section is ignored except that it MUST still parse.
    #[serde(default)]
    pub enabled: bool,
    /// Addresses the station listens on for routers dialing in. A station must
    /// listen somewhere, so this is required when `enabled`. The RFC fixes no
    /// port; 11019 is the common choice.
    #[serde(default)]
    pub listen: Vec<SocketAddr>,
    /// The routers expected to export BMP here. Acts as an allow-list: the
    /// runtime accepts sessions only from these addresses. Empty means the
    /// operator relies on network ACLs alone, which is allowed but noted.
    #[serde(default, rename = "router")]
    pub routers: Vec<BmpRouterConfig>,
    /// The id the station's routes are offered under as a query source, as it
    /// appears in URLs and the API. MUST NOT collide with a router id or the
    /// local BGP session's id. Every monitored router feeds this one source
    /// until per-router RIBs (a later slice) let each be its own.
    #[serde(default = "default_source_id")]
    pub source_id: String,
    /// The name shown for the station in the source selector. Operator data,
    /// like a router's name, so it is not a translated UI string.
    #[serde(default = "default_source_name")]
    pub source_name: String,
}

fn default_source_id() -> String {
    "bmp-local".to_string()
}

fn default_source_name() -> String {
    "BMP monitored routes".to_string()
}

impl Default for BmpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: Vec::new(),
            routers: Vec::new(),
            source_id: default_source_id(),
            source_name: default_source_name(),
        }
    }
}

/// One router expected to export BMP to NOGGlass.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BmpRouterConfig {
    /// Stable identifier, used in the API and logs.
    pub id: String,
    /// The address the router connects from.
    pub address: IpAddr,
    /// Optional human note shown in the interface.
    #[serde(default)]
    pub description: Option<String>,
}

impl BmpSettings {
    /// Checks the section makes sense before the process starts.
    ///
    /// `Ok` immediately when disabled — a turned-off section never blocks boot.
    /// When enabled, a station needs somewhere to listen, and every configured
    /// router needs a distinct id and address (fail closed).
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        if self.listen.is_empty() {
            return Err(
                "bmp.enabled is true but bmp.listen is empty; a BMP station must listen somewhere"
                    .to_string(),
            );
        }
        if self.source_id.trim().is_empty() {
            return Err("bmp.source_id must not be empty".to_string());
        }

        let mut ids = BTreeSet::new();
        let mut addresses = BTreeSet::new();
        for router in &self.routers {
            if router.id.trim().is_empty() {
                return Err("a [[bmp.router]] has an empty id".to_string());
            }
            if !ids.insert(router.id.clone()) {
                return Err(format!(
                    "bmp.router id '{}' is used more than once",
                    router.id
                ));
            }
            if !addresses.insert(router.address) {
                return Err(format!(
                    "bmp.router address '{}' is configured more than once",
                    router.address
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> BmpSettings {
        toml::from_str(source).expect("valid toml")
    }

    #[test]
    fn the_default_is_disabled_and_empty() {
        let bmp = BmpSettings::default();
        assert!(!bmp.enabled);
        assert!(bmp.listen.is_empty());
        assert!(bmp.routers.is_empty());
        assert_eq!(bmp.source_id, "bmp-local");
        assert_eq!(bmp.source_name, "BMP monitored routes");
        assert!(bmp.validate().is_ok());
    }

    #[test]
    fn an_empty_source_id_is_refused() {
        let bmp = parse(
            r#"
            enabled = true
            listen = ["[::]:11019"]
            source_id = "   "
            "#,
        );
        assert!(bmp.validate().unwrap_err().contains("source_id"));
    }

    #[test]
    fn a_disabled_section_validates_with_nothing_else() {
        let bmp = parse("enabled = false");
        assert!(bmp.validate().is_ok());
    }

    #[test]
    fn a_well_formed_enabled_station_validates() {
        let bmp = parse(
            r#"
            enabled = true
            listen = ["[::]:11019"]

            [[router]]
            id = "edge01"
            address = "192.0.2.2"
            "#,
        );
        assert!(bmp.enabled);
        assert_eq!(bmp.listen.len(), 1);
        assert_eq!(bmp.routers.len(), 1);
        assert_eq!(bmp.routers[0].id, "edge01");
        bmp.validate().expect("should be valid");
    }

    #[test]
    fn enabled_without_a_listen_address_is_refused() {
        let bmp = parse("enabled = true");
        assert!(bmp.validate().unwrap_err().contains("listen"));
    }

    #[test]
    fn duplicate_router_addresses_are_refused() {
        let bmp = parse(
            r#"
            enabled = true
            listen = ["[::]:11019"]
            [[router]]
            id = "a"
            address = "192.0.2.2"
            [[router]]
            id = "b"
            address = "192.0.2.2"
            "#,
        );
        assert!(bmp.validate().unwrap_err().contains("more than once"));
    }

    #[test]
    fn an_empty_router_id_is_refused() {
        let bmp = parse(
            r#"
            enabled = true
            listen = ["[::]:11019"]
            [[router]]
            id = ""
            address = "192.0.2.2"
            "#,
        );
        assert!(bmp.validate().unwrap_err().contains("empty id"));
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        let err = toml::from_str::<BmpSettings>("enable = true").unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}
