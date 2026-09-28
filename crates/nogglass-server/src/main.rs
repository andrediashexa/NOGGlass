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
use looking_glass_core::executor::ssh::{HostKeyPolicy, SshTransport};
use looking_glass_core::executor::{Executor, Transport};
use looking_glass_core::inventory::Inventory;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{error, info, warn};

/// Where the inventory lives unless the operator says otherwise.
const DEFAULT_CONFIG: &str = "/etc/nogglass/nogglass.toml";

/// Unprivileged by default (ADR-0012): binding 80 or 443 needs a capability or
/// a proxy, and the deployment guide covers both.
const DEFAULT_LISTEN: &str = "0.0.0.0:8080";

/// Loads key-value pairs from an environment file if present.
///
/// Priority:
/// 1. Path in `NOGGLASS_ENV_FILE` if set.
/// 2. `/etc/nogglass/nogglass.env` as the standard default.
///
/// Variables already present in the process environment are never overwritten.
fn load_env_file() {
    let env_path = std::env::var("NOGGLASS_ENV_FILE")
        .unwrap_or_else(|_| "/etc/nogglass/nogglass.env".to_string());

    let content = match std::fs::read_to_string(&env_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((raw_key, raw_val)) = trimmed.split_once('=') {
            let key = raw_key.trim();
            if key.is_empty() {
                continue;
            }

            let mut val = raw_val.trim();
            if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
                || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
            {
                val = &val[1..val.len() - 1];
            }

            if std::env::var(key).is_err() {
                // SAFETY: Executed in main() prior to spawning async runtime or worker threads.
                unsafe {
                    std::env::set_var(key, val);
                }
            }
        }
    }
}

fn main() -> ExitCode {
    load_env_file();

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to initialize async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    rt.block_on(async_main())
}

async fn async_main() -> ExitCode {
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
    let pinned_count = inventory
        .routers
        .iter()
        .filter(|r| r.host_key.is_some())
        .count();
    let total_count = inventory.routers.len();
    if pinned_count == total_count && total_count > 0 {
        info!("All {total_count} routers have pinned SSH host keys");
    } else if pinned_count > 0 {
        info!(
            "{pinned_count} of {total_count} routers have pinned SSH host keys; unpinned routers connect with AcceptAny"
        );
    } else {
        warn!(
            "running with SSH HostKeyPolicy::AcceptAny — no router host keys configured in nogglass.toml. \
             In production, set 'host_key' for routers to prevent Man-in-the-Middle attacks."
        );
    }

    // NOGGlass's own BGP session(s), if configured (#171, ADR-0016). Holding
    // the runtime for the life of the process keeps the sessions up; dropping
    // it stops them. Disabled by default, so most deployments open no socket.
    let _bgp_runtime = if inventory.bgp.enabled {
        info!(
            peers = inventory.bgp.peers.len(),
            local_as = ?inventory.bgp.local_as,
            "bgp: starting NOGGlass's own BGP session(s)"
        );
        let runtime = looking_glass_core::bgp::spawn(&inventory.bgp);
        // Operational visibility while the query path that reads this RIB is
        // still being built: report how many prefixes the session has learned.
        let rib = runtime.rib();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(30));
            loop {
                ticker.tick().await;
                let prefixes = rib.read().await.prefix_count();
                info!(prefixes, "bgp: local RIB");
            }
        });
        Some(runtime)
    } else {
        None
    };

    let _bmp_station = if inventory.bmp.enabled {
        info!(
            listen = ?inventory.bmp.listen,
            routers = inventory.bmp.routers.len(),
            "bmp: starting the BMP station"
        );
        let station = looking_glass_core::bmp::spawn(&inventory.bmp).await;
        // Same operational visibility as the BGP session: report how many
        // routers are connected and how many prefixes they have filled in.
        let ribs = station.ribs();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(30));
            loop {
                ticker.tick().await;
                let mut prefixes = 0;
                let keys = ribs.live_keys().await;
                for key in &keys {
                    if let Some(rib) = ribs.get(key).await {
                        prefixes += rib.read().await.prefix_count();
                    }
                }
                info!(routers = keys.len(), prefixes, "bmp: station RIBs");
            }
        });
        Some(station)
    } else {
        None
    };

    let transport: Arc<dyn Transport> = Arc::new(SshTransport::new(
        Duration::from_secs(15),
        Duration::from_secs(30),
        HostKeyPolicy::AcceptAny,
    ));
    let mut executor = Executor::new(inventory.clone(), Arc::new(BUILTIN.clone()), transport);

    let rpki = inventory.rpki.to_config();
    let rpki_enricher = if !matches!(
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
        // One enricher shared by the SSH executor and the local-RIB endpoints, so
        // a BGP-session or BMP-monitored route is validated just like a scraped
        // one.
        Some(Arc::new(looking_glass_core::rpki::Enricher::new(rpki)))
    } else {
        None
    };
    if let Some(enricher) = &rpki_enricher {
        executor = executor.with_rpki(enricher.clone());
    }
    let executor = Arc::new(executor);

    // Rate limiting protects the routers from one visitor; the executor's
    // concurrency caps protect them from all visitors at once. Both are needed.
    let (client_address, rejected) = inventory.rate_limit.to_client_address();
    for entry in rejected {
        warn!(entry = %entry, "trusted_proxies entry is not an address; ignoring it");
    }
    if !inventory.rate_limit.enabled {
        warn!(
            "rate limiting is OFF: a single visitor can spend as much router \
             control-plane CPU as they like"
        );
    } else if std::env::var("NOGGLASS_BEHIND_PROXY").is_ok() && !client_address.trusts_any_proxy() {
        warn!(
            "a proxy is declared but trusted_proxies is empty, so every visitor \
             counts as the proxy and they share one allowance; list the proxy \
             addresses under [rate_limit]"
        );
    }

    let limiter = inventory.rate_limit.enabled.then(|| {
        Arc::new(looking_glass_core::ratelimit::RateLimiter::new(
            inventory.rate_limit.to_limit(),
        ))
    });

    // Buckets for visitors who never come back would otherwise accumulate.
    if let Some(limiter) = limiter.clone() {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                ticker.tick().await;
                limiter.evict_idle();
            }
        });
    }

    if inventory.global_view.enabled {
        warn!(
            "the global-view comparison is enabled: queried prefixes are sent to \
              RIPEstat. Say so in your privacy notice."
        );
    }
    let global_view = Arc::new(looking_glass_core::global_view::GlobalViewLookup::new(
        inventory.global_view.enabled,
        std::time::Duration::from_millis(inventory.global_view.timeout_ms),
    ));

    let captcha_secret = match std::env::var("NOGGLASS_CAPTCHA_SECRET") {
        Ok(s) => s,
        Err(_) => {
            warn!(
                "NOGGLASS_CAPTCHA_SECRET not set; generated ephemeral in-memory secret. \
                 CAPTCHA tokens will not persist across restarts or multiple replicas. \
                 Set NOGGLASS_CAPTCHA_SECRET in the environment for production deployments."
            );
            let mut bytes = [0u8; 32];
            let mut rng = rand::thread_rng();
            rand::RngCore::fill_bytes(&mut rng, &mut bytes);
            hex::encode(bytes)
        }
    };

    let ui_state =
        Arc::new(ui::UiState::from_settings(&inventory.ui).map_err(|e| {
            format!("{e}\nNOGGlass will not start with an invalid UI configuration.")
        })?);

    let app = api::routes(AppState {
        executor,
        inventory,
        version,
        limiter,
        client_address: Arc::new(client_address),
        global_view,
        captcha_secret,
        used_captchas: Arc::new(Mutex::new(HashSet::new())),
        bgp_rib: _bgp_runtime.as_ref().map(|runtime| runtime.rib()),
        bgp_states: _bgp_runtime.as_ref().map(|runtime| runtime.states()),
        bmp_ribs: _bmp_station.as_ref().map(|station| station.ribs()),
        rpki: rpki_enricher,
    })
    .merge(ui::routes(ui_state))
    .layer(axum::middleware::from_fn(security_headers_middleware))
    .layer(tower_http::trace::TraceLayer::new_for_http())
    .layer(tower_http::compression::CompressionLayer::new());

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    info!("listening on http://{listen}");

    // ConnectInfo carries the peer address, which the rate limiter needs.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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

/// Injects standard HTTP security headers across all responses.
async fn security_headers_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        axum::http::HeaderValue::from_static("DENY"),
    );
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        axum::http::header::CONTENT_SECURITY_POLICY,
        axum::http::HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self'; frame-ancestors 'none';",
        ),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_env_file_reads_key_value_pairs_and_strips_quotes() {
        let env_file_path =
            std::env::temp_dir().join(format!("nogglass_test_{}.env", rand::random::<u64>()));
        std::fs::write(
            &env_file_path,
            "# Comment line\n\
             TEST_NOGGLASS_KEY_ONE=val1\n\
             TEST_NOGGLASS_KEY_TWO=\"quoted_val\"\n\
             TEST_NOGGLASS_KEY_THREE='single_quoted'\n\
             TEST_NOGGLASS_KEY_FOUR = spaced_val \n",
        )
        .unwrap();

        unsafe {
            std::env::set_var("NOGGLASS_ENV_FILE", env_file_path.to_str().unwrap());
        }

        load_env_file();

        assert_eq!(std::env::var("TEST_NOGGLASS_KEY_ONE").unwrap(), "val1");
        assert_eq!(
            std::env::var("TEST_NOGGLASS_KEY_TWO").unwrap(),
            "quoted_val"
        );
        assert_eq!(
            std::env::var("TEST_NOGGLASS_KEY_THREE").unwrap(),
            "single_quoted"
        );
        assert_eq!(
            std::env::var("TEST_NOGGLASS_KEY_FOUR").unwrap(),
            "spaced_val"
        );

        let _ = std::fs::remove_file(&env_file_path);
    }
}
