//! The BMP station NOGGlass runs (issue #172, [ADR-0016]).
//!
//! Operators point their routers' BMP exporters at NOGGlass; it listens, parses
//! the RFC 7854 stream into the normalised path model, and serves the monitored
//! routes. Observe-only by the protocol's nature — the station never sends
//! routing state to a router.
//!
//! - [`config`] is the `[bmp]` section of the inventory.
//! - [`mapping`] turns received BMP messages into changes on the shared RIB,
//!   reusing the BGP mapper so a BMP-seen route reads like any other.
//!
//! The station runtime (listener, per-session parser, per-peer RIBs) lands in a
//! later change.
//!
//! [ADR-0016]: ../../docs/adr/0016-one-rust-engine-for-bgp-session-and-bmp.md

pub mod config;
pub mod mapping;

pub use config::{BmpRouterConfig, BmpSettings};
pub use mapping::{apply_bmp_message, paths_from_route_monitoring};
