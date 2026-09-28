//! The BMP station NOGGlass runs (issue #172, [ADR-0016]).
//!
//! Operators point their routers' BMP exporters at NOGGlass; it listens, parses
//! the RFC 7854 stream into the normalised path model, and serves the monitored
//! routes. Observe-only by the protocol's nature — the station never sends
//! routing state to a router.
//!
//! - [`config`] is the `[bmp]` section of the inventory.
//! - [`mapping`] turns received BMP messages into RIB changes, reusing the BGP
//!   mapper so a BMP-seen route reads like any other.
//! - [`rib`] holds one [`rib::BmpRibs`] entry per monitored router, so one
//!   router's view is isolated from another's and a disconnected router stops
//!   answering rather than serving a stale table.
//! - [`runtime`] is the listener: it accepts routers, parses their stream, and
//!   feeds it into that router's RIB.
//!
//! [ADR-0016]: ../../docs/adr/0016-one-rust-engine-for-bgp-session-and-bmp.md

pub mod config;
pub mod mapping;
pub mod rib;
pub mod runtime;
pub mod summary;

pub use config::{BmpRouterConfig, BmpSettings};
pub use mapping::{apply_bmp_message, paths_from_route_monitoring, record_peer_event};
pub use rib::{BmpPeer, BmpRibs, BmpSession, SharedPeers};
pub use runtime::{spawn, BmpStation};
pub use summary::bmp_summary;
