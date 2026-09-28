//! The BMP station NOGGlass runs (issue #172, [ADR-0016]).
//!
//! Operators point their routers' BMP exporters at NOGGlass; it listens, parses
//! the RFC 7854 stream into the normalised path model, and serves the monitored
//! routes. Observe-only by the protocol's nature — the station never sends
//! routing state to a router.
//!
//! - [`config`] is the `[bmp]` section of the inventory.
//!
//! The station runtime (listener, per-session parser, per-peer RIBs) and the
//! mapping of BMP-embedded UPDATEs into the shared model land in later changes.
//!
//! [ADR-0016]: ../../docs/adr/0016-one-rust-engine-for-bgp-session-and-bmp.md

pub mod config;

pub use config::{BmpRouterConfig, BmpSettings};
