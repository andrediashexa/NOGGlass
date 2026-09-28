//! The BGP session NOGGlass holds itself (issue #171, [ADR-0016]).
//!
//! An operator points their own router at NOGGlass — or lets NOGGlass dial it —
//! and NOGGlass keeps the received routes in a local RIB, answering the looking
//! glass from that live table instead of a login per query. The engine is
//! NetGauze, embedded in the binary, so routes arrive as typed structures and
//! never as text to scrape.
//!
//! - [`config`] is the `[bgp]` section of the inventory.
//! - [`mapping`] turns a received NetGauze UPDATE into the normalised
//!   [`BgpPath`](crate::driver::BgpPath) model, so a route learned by BGP reads
//!   the same way as one scraped over SSH.
//! - [`rib`] holds the routes in memory, one Adj-RIB-In per peer, and answers
//!   prefix queries.
//!
//! The session runtime (the NetGauze supervisor and per-peer event loop) that
//! drives [`mapping`] into the [`rib`] lands in a later change.
//!
//! [ADR-0016]: ../../docs/adr/0016-one-rust-engine-for-bgp-session-and-bmp.md

pub mod config;
pub mod mapping;
pub mod rib;

pub use config::{BgpPeerConfig, BgpSettings};
pub use mapping::{paths_from_update, withdrawn_from_update};
pub use rib::LocalRib;
