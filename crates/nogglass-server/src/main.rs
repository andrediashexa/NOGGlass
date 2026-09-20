//! NOGGlass: a multi-vendor looking glass in one binary.
//!
//! Startup is deliberately strict. The inventory is read and validated, every
//! secret it names is checked, and the process exits rather than starting with
//! a configuration it does not understand — a looking glass that starts broken
//! answers visitors with surprises.

mod api;
mod ui;

use api::{AppState, VersionInfo};
use looking_glass_core::catalogue::BUILTIN;
use looking_glass_core::executor::ssh::SshTransport;
use looking_glass_core::executor::{Executor, Transport};
use looking_glass_core::inventory::Inventory;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Where the inventory lives unless the operator says otherwise.
const DEFAULT_CONFIG: &str = "/etc/nogglass/nogglass.toml";

/// Unprivileged by default (ADR-0012): binding 80 or 443 needs a capability or
/// a proxy, and the deployment guide covers both.
const DEFAULT_LISTEN: &str = "0.0.0.0:8080";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("NOGGLASS_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            error!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let version = VersionInfo::from_build();
    info!(
        version = %version.version,
        commit = %version.commit,
        "starting NOGGlass"
    );

    let config_path =
        std::env::var("NOGGLASS_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let inventory = Inventory::load(&config_path)
        .map_err(|e| format!("{e}\nNOGGlass will not start with an inventory it cannot use."))?;

    // Checked at startup rather than on the first query, so a missing password
    // is found by the operator deploying it and not by a visitor.
    inventory
        .check_secrets(|name| std::env::var(name).ok())
        .map_err(|e| e.to_string())?;

    info!(
        routers = inventory.routers.len(),
        config = %config_path,
        "inventory loaded"
    );
    for router in &inventory.routers {
        if router.is_mock() {
            warn!(
                router = %router.id,
                "this router serves fabricated data; the interface says so"
            );
        }
    }

    let listen: SocketAddr = std::env::var("NOGGLASS_HTTP_ADDR")
        .unwrap_or_else(|_| DEFAULT_LISTEN.to_string())
        .parse()
        .map_err(|e| format!("NOGGLASS_HTTP_ADDR is not an address: {e}"))?;

    // Saying this out loud beats a deployment that quietly serves a public
    // Looking Glass in the clear (ADR-0012, section 1.3).
    if std::env::var("NOGGLASS_BEHIND_PROXY").is_err() && !listen.ip().is_loopback() {
        warn!(
            "listening on {listen} without TLS and without NOGGLASS_BEHIND_PROXY set — \
             put a reverse proxy in front before exposing this to the Internet"
        );
    }

    let inventory = Arc::new(inventory);
    let transport: Arc<dyn Transport> = Arc::new(SshTransport::default());
    let mut executor = Executor::new(inventory.clone(), Arc::new(BUILTIN.clone()), transport);

    let rpki = inventory.rpki.to_config();
    if !matches!(
        rpki.validator,
        looking_glass_core::rpki::Validator::Disabled
    ) {
        info!(validator = ?rpki.validator, "RPKI fallback enabled");
        if matches!(
            rpki.validator,
            looking_glass_core::rpki::Validator::RipeStat
        ) {
            warn!(
                "RPKI fallback uses RIPEstat: queried prefixes and origin AS numbers \
                 leave this network. Say so in your privacy notice, or configure \
                 validator_url to point at your own validator."
            );
        }
        executor = executor.with_rpki(Arc::new(looking_glass_core::rpki::Enricher::new(rpki)));
    }
    let executor = Arc::new(executor);

    let app = api::routes(AppState {
        executor,
        inventory,
        version,
    })
    .merge(ui::routes())
    .layer(tower_http::trace::TraceLayer::new_for_http())
    .layer(tower_http::compression::CompressionLayer::new());

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    info!("listening on http://{listen}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server stopped: {e}"))
}

/// Finishes in-flight queries before exiting, so a restart does not cut a
/// visitor's answer in half.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutting down");
}
