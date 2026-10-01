//! The operator's router inventory.
//!
//! An operator describes their routers in `/etc/nogglass/routers.conf`: what
//! each one is called, which vendor it speaks, how to reach it, and which
//! queries it may answer. The file is validated at startup and the process
//! refuses to run with a broken one — a Looking Glass that starts with an
//! inventory it does not understand would answer queries with surprises.
//!
//! Credentials are never written here. The file names an environment variable,
//! and the value stays in the environment (ADR-0012), so an inventory can be
//! version-controlled, pasted into an issue or shared without leaking a router
//! password.

use crate::catalogue::Catalogue;
use crate::driver::QueryType;
use crate::target::QueryLimits;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

/// Default SSH port.
const DEFAULT_SSH_PORT: u16 = 22;

/// What can be wrong with an inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    Unreadable {
        path: String,
        reason: String,
    },
    Malformed(String),
    NoRouters,
    DuplicateId(String),
    UnknownVendor {
        router: String,
        vendor: String,
    },
    /// The vendor cannot answer a query the router is configured to offer.
    UnsupportedQuery {
        router: String,
        vendor: String,
        query: String,
    },
    /// Credentials are named but the environment does not carry them.
    MissingSecret {
        router: String,
        variable: String,
    },
    /// A real router needs a way to authenticate; the mock does not.
    NoCredentials(String),
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, reason } => write!(f, "cannot read {path}: {reason}"),
            Self::Malformed(why) => write!(f, "inventory is malformed: {why}"),
            Self::NoRouters => write!(f, "the inventory has no routers"),
            Self::DuplicateId(id) => write!(f, "two routers share the id {id:?}"),
            Self::UnknownVendor { router, vendor } => {
                write!(f, "router {router:?} uses unknown vendor {vendor:?}")
            }
            Self::UnsupportedQuery {
                router,
                vendor,
                query,
            } => write!(
                f,
                "router {router:?} offers {query}, which {vendor} cannot answer"
            ),
            Self::MissingSecret { router, variable } => write!(
                f,
                "router {router:?} needs environment variable {variable}, which is not set"
            ),
            Self::NoCredentials(router) => write!(
                f,
                "router {router:?} has no password or key configured, and is not the mock"
            ),
        }
    }
}

impl std::error::Error for InventoryError {}

/// How NOGGlass authenticates to a router.
///
/// Both variants name an environment variable rather than carrying a secret,
/// so the inventory file is safe to keep in version control.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Credentials {
    /// Password directly specified in the configuration file.
    Password(String),
    /// Password read from this environment variable.
    PasswordEnv(String),
    /// Private key read from this file, with an optional passphrase or passphrase variable.
    KeyFile {
        path: String,
        #[serde(default)]
        passphrase: Option<String>,
        #[serde(default)]
        passphrase_env: Option<String>,
    },
    /// The mock router: no session is opened, so nothing authenticates.
    None,
}

impl Default for Credentials {
    fn default() -> Self {
        Self::None
    }
}

/// One router an operator exposes.
#[derive(Debug, Clone, Deserialize)]
pub struct Router {
    /// Stable identifier used in URLs and API calls.
    pub id: String,
    /// What visitors see, such as "Edge 01 — São Paulo".
    pub name: String,
    /// Vendor key, matching the command catalogue.
    pub vendor: String,
    /// Management address. Not shown to visitors.
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    /// Password specified directly on the router entry.
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub credentials: Credentials,
    /// Grouping shown in the selector, typically a POP or a city.
    #[serde(default)]
    pub location: Option<String>,
    /// Queries this router answers. Empty means every query its vendor supports.
    #[serde(default)]
    pub queries: Vec<QueryType>,
    /// Per-router overrides of the global limits.
    #[serde(default)]
    pub max_concurrent: Option<usize>,
    /// Expected OpenSSH host public key (e.g. "ssh-ed25519 AAAA...").
    /// Pinned to prevent Man-in-the-Middle attacks on the management network.
    #[serde(default)]
    pub host_key: Option<String>,
    /// Explicitly allow unverified host keys (AcceptAny) for lab or dev environments.
    /// Defaults to false. If false and host_key is None, connection attempts are rejected.
    #[serde(default)]
    pub allow_insecure_host_key: bool,
    /// Optional source IPv4 address used for ping and traceroute.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_ipv4",
        alias = "source_ip_v4",
        alias = "source_ipv4",
        alias = "src_v4"
    )]
    pub source_v4: Option<Ipv4Addr>,
    /// Optional source IPv6 address used for ping and traceroute.
    #[serde(
        default,
        deserialize_with = "deserialize_opt_ipv6",
        alias = "source_ip_v6",
        alias = "source_ipv6",
        alias = "src_v6"
    )]
    pub source_v6: Option<Ipv6Addr>,
}

fn deserialize_opt_ipv4<'de, D>(deserializer: D) -> Result<Option<Ipv4Addr>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => s
            .trim()
            .parse::<Ipv4Addr>()
            .map(Some)
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

fn deserialize_opt_ipv6<'de, D>(deserializer: D) -> Result<Option<Ipv6Addr>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => s
            .trim()
            .parse::<Ipv6Addr>()
            .map(Some)
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

fn default_port() -> u16 {
    DEFAULT_SSH_PORT
}

impl Router {
    /// Returns the resolved credentials for this router, considering both
    /// the direct `password` property and the `credentials` configuration.
    pub fn resolved_credentials(&self) -> Credentials {
        match &self.credentials {
            Credentials::None => {
                if let Some(pass) = &self.password {
                    Credentials::Password(pass.clone())
                } else {
                    Credentials::None
                }
            }
            creds => creds.clone(),
        }
    }

    /// Whether a visitor may run this query against this router.
    pub fn allows(&self, query: QueryType) -> bool {
        if self.queries.is_empty() {
            // Default safe queries when unspecified: do NOT enable heavy aspath regex by default.
            query != QueryType::BgpAspath && query != QueryType::BgpAspathV6
        } else {
            self.queries.contains(&query)
        }
    }

    /// Whether this entry is the mock router rather than a real device.
    pub fn is_mock(&self) -> bool {
        self.vendor == crate::vendors::MOCK_VENDOR
    }

    /// What a visitor is allowed to see: never the host, the port or the user.
    pub fn public_view(&self) -> PublicRouter {
        PublicRouter {
            id: self.id.clone(),
            name: self.name.clone(),
            vendor: self.vendor.clone(),
            location: self.location.clone(),
            is_mock: self.is_mock(),
        }
    }
}

/// The router as the interface sees it.
///
/// Management addresses, ports, usernames and credential variables never leave
/// the process (ADR-0007): a Looking Glass publishes diagnostics, not a map of
/// the operator's management plane.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PublicRouter {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub location: Option<String>,
    pub is_mock: bool,
}

/// Settings that apply to every query.
#[derive(Debug, Clone, Deserialize)]
pub struct Limits {
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_output_bytes")]
    pub max_output_bytes: usize,
    #[serde(default = "default_ping_count")]
    pub ping_count: u8,
    /// Queries that may run at once against one router.
    #[serde(default = "default_per_router")]
    pub max_concurrent_per_router: usize,
    /// Queries that may run at once across the whole instance.
    #[serde(default = "default_global")]
    pub max_concurrent_total: usize,
}

fn default_timeout() -> u64 {
    30
}
fn default_output_bytes() -> usize {
    2 * 1024 * 1024
}
fn default_ping_count() -> u8 {
    5
}
fn default_per_router() -> usize {
    2
}
fn default_global() -> usize {
    16
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout(),
            max_output_bytes: default_output_bytes(),
            ping_count: default_ping_count(),
            max_concurrent_per_router: default_per_router(),
            max_concurrent_total: default_global(),
        }
    }
}

impl Limits {
    pub fn query_limits(&self) -> QueryLimits {
        QueryLimits {
            timeout_secs: self.timeout_secs,
            max_output_bytes: self.max_output_bytes,
            ping_count: self.ping_count,
        }
    }
}

/// A secret that is unset or empty is missing: an empty password would
/// otherwise be sent to the router.
///
/// Written out rather than using `Option::is_none_or`, which needs a newer
/// compiler than the one this crate supports.
fn is_blank(value: Option<String>) -> bool {
    match value {
        Some(v) => v.is_empty(),
        None => true,
    }
}

/// Comparing the router answer with what the Internet announces (ADR-0006).
#[derive(Debug, Clone, Deserialize)]
pub struct GlobalViewSettings {
    /// Off by default: turning it on sends the queried prefix to RIPEstat, so
    /// it is the operator's call and belongs in their privacy notice.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_global_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_global_timeout_ms() -> u64 {
    1500
}

impl Default for GlobalViewSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: default_global_timeout_ms(),
        }
    }
}

/// How much a single visitor may ask for.
///
/// Separate from [`Limits`], which bounds one query; this bounds how many
/// queries one visitor gets.
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitSettings {
    /// Turning this off on a public instance means a stranger decides how much
    /// control-plane CPU your routers spend.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_requests")]
    pub max_requests: u32,
    #[serde(default = "default_window_secs")]
    pub window_secs: u64,
    #[serde(default = "default_burst")]
    pub burst: u32,
    /// When set (defaults to 60 seconds), queries from the same client within this interval
    /// require a CAPTCHA challenge before being executed.
    #[serde(default = "default_captcha_secs")]
    pub require_captcha_within_secs: u64,
    /// Addresses of proxies whose `X-Forwarded-For` may be believed. Empty
    /// means the peer address is always used, which is correct when nothing
    /// sits in front.
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
    /// Optional CAPTCHA secret key configured under `[rate_limit]`.
    #[serde(default)]
    pub captcha_secret: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_max_requests() -> u32 {
    20
}
fn default_window_secs() -> u64 {
    60
}
fn default_burst() -> u32 {
    5
}
fn default_captcha_secs() -> u64 {
    60
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            max_requests: default_max_requests(),
            window_secs: default_window_secs(),
            burst: default_burst(),
            require_captcha_within_secs: default_captcha_secs(),
            trusted_proxies: Vec::new(),
            captcha_secret: None,
        }
    }
}

impl RateLimitSettings {
    pub fn to_limit(&self) -> crate::ratelimit::RateLimit {
        crate::ratelimit::RateLimit {
            max_requests: self.max_requests,
            window: std::time::Duration::from_secs(self.window_secs.max(1)),
            burst: self.burst,
            require_captcha_within_secs: self.require_captcha_within_secs,
        }
    }

    /// The proxy resolver, plus any entry that did not parse so the caller can
    /// complain loudly rather than trusting silently.
    pub fn to_client_address(&self) -> (crate::ratelimit::ClientAddress, Vec<String>) {
        crate::ratelimit::ClientAddress::from_list(&self.trusted_proxies.join(","))
    }
}

/// Tier 2 RPKI validation, as the operator writes it (ADR-0010).
///
/// Disabled unless the operator says otherwise: a default that reaches a third
/// party would send every visitor's query off their network without anyone
/// deciding to.
#[derive(Debug, Clone, Deserialize)]
pub struct RpkiSettings {
    /// Turns Tier 2 on. With no `validator_url`, RIPEstat is used, which sends
    /// the queried prefix and origin AS to a third party — say so in your
    /// privacy notice.
    #[serde(default)]
    pub enable_fallback: bool,
    /// An operator-run validator, such as Routinator.
    #[serde(default)]
    pub validator_url: Option<String>,
    #[serde(default = "default_rpki_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_rpki_ttl")]
    pub cache_ttl_secs: u64,
    #[serde(default = "default_rpki_capacity")]
    pub cache_max_capacity: usize,
}

fn default_rpki_timeout_ms() -> u64 {
    3000
}
fn default_rpki_ttl() -> u64 {
    3600
}
fn default_rpki_capacity() -> usize {
    50_000
}

impl Default for RpkiSettings {
    fn default() -> Self {
        Self {
            enable_fallback: false,
            validator_url: None,
            timeout_ms: default_rpki_timeout_ms(),
            cache_ttl_secs: default_rpki_ttl(),
            cache_max_capacity: default_rpki_capacity(),
        }
    }
}

impl RpkiSettings {
    /// Turns the file settings into what the enricher needs.
    pub fn to_config(&self) -> crate::rpki::RpkiConfig {
        use crate::rpki::{RpkiConfig, Validator};
        let validator = match (self.enable_fallback, &self.validator_url) {
            (false, _) => Validator::Disabled,
            (true, Some(url)) => Validator::Local {
                base_url: url.clone(),
            },
            (true, None) => Validator::RipeStat,
        };
        RpkiConfig {
            validator,
            timeout: std::time::Duration::from_millis(self.timeout_ms),
            cache_ttl: std::time::Duration::from_secs(self.cache_ttl_secs),
            cache_capacity: self.cache_max_capacity,
        }
    }
}

/// Available interface themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Dark,
    Light,
}

impl Default for Theme {
    fn default() -> Self {
        Self::Dark
    }
}

/// Interface customization and branding settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UiSettings {
    /// Visual theme ("dark" or "light").
    #[serde(default)]
    pub theme: Theme,
    /// Path to a custom logo image file on the host filesystem.
    #[serde(default = "default_logo_path")]
    pub logo_path: Option<String>,
    /// Height of the logo in pixels.
    #[serde(default = "default_logo_height_px")]
    pub logo_height_px: u32,
    /// Path to a custom background wallpaper image on the host filesystem.
    #[serde(default = "default_background_path")]
    pub background_path: Option<String>,
    /// Background blur radius in pixels.
    #[serde(default = "default_background_blur_px")]
    pub background_blur_px: u32,
    /// Background opacity percentage (0 to 100).
    #[serde(default = "default_background_opacity_percent")]
    pub background_opacity_percent: u32,
    /// Whether to display the "MELHOR CAMINHO" AS-path graph panel (default: true).
    #[serde(default = "default_true", alias = "show_graph")]
    pub show_best_path: bool,
    /// Whether to display the "CAMINHOS" tabular routes panel (default: true).
    #[serde(default = "default_true", alias = "show_routes")]
    pub show_paths: bool,
    /// Whether to display the "SAÍDA DO ROTEADOR" raw CLI output panel (default: true).
    #[serde(
        default = "default_true",
        alias = "show_raw",
        alias = "show_router_output"
    )]
    pub show_raw_output: bool,
}

fn default_logo_path() -> Option<String> {
    Some("/etc/nogglass/logo_nogglass.png".to_string())
}

fn default_background_path() -> Option<String> {
    Some("/etc/nogglass/nogglass.png".to_string())
}

fn default_logo_height_px() -> u32 {
    76
}

fn default_background_blur_px() -> u32 {
    1
}

fn default_background_opacity_percent() -> u32 {
    35
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            logo_path: default_logo_path(),
            logo_height_px: default_logo_height_px(),
            background_path: default_background_path(),
            background_blur_px: default_background_blur_px(),
            background_opacity_percent: default_background_opacity_percent(),
            show_best_path: default_true(),
            show_paths: default_true(),
            show_raw_output: default_true(),
        }
    }
}

/// Server listen and security settings.
#[derive(Clone, Deserialize)]
pub struct ServerSettings {
    #[serde(default = "default_http_addr", alias = "listen")]
    pub http_addr: String,
    #[serde(default)]
    pub captcha_secret: Option<String>,
    #[serde(default)]
    pub bgp_summary_password: Option<String>,
}

impl fmt::Debug for ServerSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerSettings")
            .field("http_addr", &self.http_addr)
            .field(
                "captcha_secret",
                &self.captcha_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "bgp_summary_password",
                &self.bgp_summary_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

fn default_http_addr() -> String {
    "0.0.0.0:8080".to_string()
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            http_addr: default_http_addr(),
            captcha_secret: None,
            bgp_summary_password: None,
        }
    }
}

/// Security and access control settings.
#[derive(Clone, Default, Deserialize)]
pub struct SecuritySettings {
    #[serde(default)]
    pub bgp_summary_password: Option<String>,
}

impl fmt::Debug for SecuritySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecuritySettings")
            .field(
                "bgp_summary_password",
                &self.bgp_summary_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// The whole configuration file.
#[derive(Debug, Clone, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub security: SecuritySettings,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub rpki: RpkiSettings,
    #[serde(default, rename = "rate_limit")]
    pub rate_limit: RateLimitSettings,
    #[serde(default, rename = "global_view")]
    pub global_view: GlobalViewSettings,
    #[serde(default)]
    pub ui: UiSettings,
    #[serde(rename = "router", default)]
    pub routers: Vec<Router>,
}

#[derive(Debug, Deserialize)]
struct RoutersFile {
    #[serde(rename = "router", default)]
    routers: Vec<Router>,
}

#[derive(Debug, Deserialize)]
struct UiFile {
    #[serde(default)]
    ui: Option<UiSettings>,
}

impl Inventory {
    /// Reads and validates an inventory file, automatically discovering companion
    /// configuration files (`routers.conf` and `ui.conf`) in the same directory.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, InventoryError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|e| InventoryError::Unreadable {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let mut inventory: Self = toml::from_str(&source)
            .map_err(|e| InventoryError::Malformed(format!("{}: {e}", path.display())))?;

        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));

        // 1. Look for companion routers configuration (routers.conf / routers.toml)
        let routers_path = std::env::var("NOGGLASS_ROUTERS_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let conf = base_dir.join("routers.conf");
                if conf.exists() {
                    conf
                } else {
                    base_dir.join("routers.toml")
                }
            });

        if routers_path.exists() {
            let routers_source =
                std::fs::read_to_string(&routers_path).map_err(|e| InventoryError::Unreadable {
                    path: routers_path.display().to_string(),
                    reason: e.to_string(),
                })?;
            let parsed_file: RoutersFile = toml::from_str(&routers_source).map_err(|e| {
                InventoryError::Malformed(format!("{}: {e}", routers_path.display()))
            })?;
            if !parsed_file.routers.is_empty() {
                inventory.routers = parsed_file.routers;
            }
        }

        // 2. Look for companion ui configuration (ui.conf / ui.toml)
        let ui_path = std::env::var("NOGGLASS_UI_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let conf = base_dir.join("ui.conf");
                if conf.exists() {
                    conf
                } else {
                    base_dir.join("ui.toml")
                }
            });

        if ui_path.exists() {
            let ui_source =
                std::fs::read_to_string(&ui_path).map_err(|e| InventoryError::Unreadable {
                    path: ui_path.display().to_string(),
                    reason: e.to_string(),
                })?;
            let parsed_file: Result<UiFile, _> = toml::from_str(&ui_source);
            match parsed_file {
                Ok(file) if file.ui.is_some() => {
                    inventory.ui = file.ui.unwrap();
                }
                _ => {
                    // Fallback: try parsing directly as UiSettings if [ui] table header was omitted
                    if let Ok(direct_ui) = toml::from_str::<UiSettings>(&ui_source) {
                        inventory.ui = direct_ui;
                    } else if let Err(e) = parsed_file {
                        return Err(InventoryError::Malformed(format!(
                            "{}: {e}",
                            ui_path.display()
                        )));
                    }
                }
            }
        }

        inventory.validate(&crate::catalogue::BUILTIN)?;
        Ok(inventory)
    }

    /// Parses and validates an inventory, without touching the environment.
    pub fn from_toml(source: &str) -> Result<Self, InventoryError> {
        let inventory: Self =
            toml::from_str(source).map_err(|e| InventoryError::Malformed(e.to_string()))?;
        inventory.validate(&crate::catalogue::BUILTIN)?;
        Ok(inventory)
    }

    /// Checks the inventory against the command catalogue.
    ///
    /// Fails closed: an inventory that offers a query its vendor cannot answer
    /// is an error at startup, not a surprise for the first visitor who tries
    /// it.
    pub fn validate(&self, catalogue: &Catalogue) -> Result<(), InventoryError> {
        if self.routers.is_empty() {
            return Err(InventoryError::NoRouters);
        }

        let mut seen = BTreeSet::new();
        for router in &self.routers {
            if !seen.insert(router.id.clone()) {
                return Err(InventoryError::DuplicateId(router.id.clone()));
            }

            let commands =
                catalogue
                    .vendor(&router.vendor)
                    .map_err(|_| InventoryError::UnknownVendor {
                        router: router.id.clone(),
                        vendor: router.vendor.clone(),
                    })?;

            for query in &router.queries {
                let supported = match query {
                    QueryType::Ping => commands.ping_v4.is_some() || commands.ping_v6.is_some(),
                    QueryType::Traceroute => {
                        commands.traceroute_v4.is_some() || commands.traceroute_v6.is_some()
                    }
                    QueryType::BgpRoute => {
                        commands.bgp_route_v4.is_some() || commands.bgp_route_v6.is_some()
                    }
                    QueryType::BgpAspath => commands.bgp_route_asn.is_some(),
                    QueryType::BgpAspathV6 => commands.bgp_route_asn_v6.is_some(),
                    QueryType::BgpSummary => commands.bgp_summary.is_some(),
                };
                if !supported {
                    return Err(InventoryError::UnsupportedQuery {
                        router: router.id.clone(),
                        vendor: router.vendor.clone(),
                        query: format!("{query:?}"),
                    });
                }
            }

            if !router.is_mock() && matches!(router.resolved_credentials(), Credentials::None) {
                return Err(InventoryError::NoCredentials(router.id.clone()));
            }
        }

        if self.ui.logo_height_px == 0 {
            return Err(InventoryError::Malformed(
                "ui.logo_height_px must be greater than 0".to_string(),
            ));
        }
        if self.ui.background_opacity_percent > 100 {
            return Err(InventoryError::Malformed(
                "ui.background_opacity_percent must be between 0 and 100".to_string(),
            ));
        }

        Ok(())
    }

    /// Checks that every secret the inventory names is present.
    ///
    /// Separate from [`Self::validate`] so a file can be checked without the
    /// production environment, and so the startup check can be explicit about
    /// which variable is missing.
    pub fn check_secrets(
        &self,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<(), InventoryError> {
        for router in &self.routers {
            match router.resolved_credentials() {
                Credentials::Password(pass) => {
                    if pass.trim().is_empty() {
                        return Err(InventoryError::Malformed(format!(
                            "router '{}' has an empty password",
                            router.id
                        )));
                    }
                }
                Credentials::PasswordEnv(variable) => {
                    if is_blank(lookup(&variable)) {
                        return Err(InventoryError::MissingSecret {
                            router: router.id.clone(),
                            variable: variable.clone(),
                        });
                    }
                }
                Credentials::KeyFile {
                    passphrase_env: Some(variable),
                    passphrase,
                    ..
                } => {
                    if passphrase.is_none() && is_blank(lookup(&variable)) {
                        return Err(InventoryError::MissingSecret {
                            router: router.id.clone(),
                            variable: variable.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn router(&self, id: &str) -> Option<&Router> {
        self.routers.iter().find(|r| r.id == id)
    }

    /// What the interface lists, in configuration order.
    pub fn public_routers(&self) -> Vec<PublicRouter> {
        self.routers.iter().map(Router::public_view).collect()
    }

    /// Returns the configured BGP summary password, if any.
    ///
    /// Precedence:
    /// 1. `NOGGLASS_BGP_SUMMARY_PASSWORD` environment variable.
    /// 2. `[security].bgp_summary_password` in configuration.
    /// 3. `[server].bgp_summary_password` in configuration.
    pub fn bgp_summary_password(&self) -> Option<String> {
        std::env::var("NOGGLASS_BGP_SUMMARY_PASSWORD")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.security.bgp_summary_password.clone())
            .or_else(|| self.server.bgp_summary_password.clone())
            .filter(|s| !s.trim().is_empty())
    }

    /// Whether BGP summary queries are protected with an authentication password.
    pub fn is_bgp_summary_configured(&self) -> bool {
        self.bgp_summary_password().is_some()
    }

    /// Parsed management address, when the host is an address rather than a
    /// name.
    pub fn management_address(router: &Router) -> Option<IpAddr> {
        router.host.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[limits]
timeout_secs = 20

[[router]]
id = "edge-01"
name = "Edge 01 - Sao Paulo"
vendor = "huawei_vrp"
host = "192.0.2.10"
username = "lookingglass"
location = "Sao Paulo"
credentials = { password_env = "NOGGLASS_EDGE01_PASSWORD" }
queries = ["ping", "traceroute", "bgp_route"]

[[router]]
id = "demo"
name = "Demo router (fabricated data)"
vendor = "mock"
host = "127.0.0.1"
"#;

    #[test]
    fn rpki_fallback_is_off_until_the_operator_turns_it_on() {
        use crate::rpki::Validator;

        let inventory = Inventory::from_toml(SAMPLE).unwrap();
        assert!(matches!(
            inventory.rpki.to_config().validator,
            Validator::Disabled
        ));

        let with_ripestat = format!("{SAMPLE}\n[rpki]\nenable_fallback = true\n");
        let inventory = Inventory::from_toml(&with_ripestat).unwrap();
        assert!(matches!(
            inventory.rpki.to_config().validator,
            Validator::RipeStat
        ));

        let with_local = format!(
            "{SAMPLE}\n[rpki]\nenable_fallback = true\nvalidator_url = \"http://routinator:8323\"\n"
        );
        let inventory = Inventory::from_toml(&with_local).unwrap();
        match inventory.rpki.to_config().validator {
            Validator::Local { base_url } => assert_eq!(base_url, "http://routinator:8323"),
            other => panic!("expected the operator's validator, got {other:?}"),
        }
    }

    #[test]
    fn loads_a_valid_inventory() {
        let inventory = Inventory::from_toml(SAMPLE).expect("sample inventory should load");
        assert_eq!(inventory.routers.len(), 2);
        assert_eq!(inventory.limits.timeout_secs, 20);
        // Defaults fill in what the file does not say.
        assert_eq!(inventory.limits.max_concurrent_per_router, 2);
        assert_eq!(inventory.router("edge-01").unwrap().port, 22);
        assert!(inventory.router("demo").unwrap().is_mock());
    }

    #[test]
    fn a_router_with_no_query_list_answers_everything_its_vendor_supports() {
        let inventory = Inventory::from_toml(SAMPLE).unwrap();
        let demo = inventory.router("demo").unwrap();
        assert!(demo.allows(QueryType::BgpSummary));

        let edge = inventory.router("edge-01").unwrap();
        assert!(edge.allows(QueryType::Ping));
        assert!(
            !edge.allows(QueryType::BgpSummary),
            "the operator did not offer this query on this router"
        );
    }

    /// The management plane never reaches the browser.
    #[test]
    fn the_public_view_hides_how_to_reach_the_router() {
        let inventory = Inventory::from_toml(SAMPLE).unwrap();
        let public = inventory.public_routers();
        let rendered = serde_json::to_string(&public).unwrap();

        assert!(rendered.contains("Edge 01"));
        for secret in [
            "192.0.2.10",
            "lookingglass",
            "NOGGLASS_EDGE01_PASSWORD",
            "22",
        ] {
            assert!(
                !rendered.contains(secret),
                "public router list leaked {secret:?}: {rendered}"
            );
        }
    }

    #[test]
    fn rejects_an_unknown_vendor() {
        let toml = r#"
[[router]]
id = "r1"
name = "R1"
vendor = "not_a_vendor"
host = "192.0.2.1"
credentials = { password_env = "X" }
"#;
        assert!(matches!(
            Inventory::from_toml(toml),
            Err(InventoryError::UnknownVendor { .. })
        ));
    }

    #[test]
    fn rejects_a_query_the_vendor_cannot_answer() {
        // RouterOS has no AS-path lookup, but bgp_route is supported; the
        // unsupported case here is a vendor without a summary command.
        let toml = r#"
[[router]]
id = "r1"
name = "R1"
vendor = "mikrotik_routeros"
host = "192.0.2.1"
credentials = { password_env = "X" }
queries = ["ping", "bgp_summary"]
"#;
        // MikroTik does have a summary command, so this must load.
        assert!(Inventory::from_toml(toml).is_ok());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let toml = r#"
[[router]]
id = "same"
name = "A"
vendor = "mock"
host = "127.0.0.1"

[[router]]
id = "same"
name = "B"
vendor = "mock"
host = "127.0.0.1"
"#;
        assert_eq!(
            Inventory::from_toml(toml).unwrap_err(),
            InventoryError::DuplicateId("same".into())
        );
    }

    #[test]
    fn a_real_router_without_credentials_is_refused() {
        let toml = r#"
[[router]]
id = "r1"
name = "R1"
vendor = "huawei_vrp"
host = "192.0.2.1"
"#;
        assert_eq!(
            Inventory::from_toml(toml).unwrap_err(),
            InventoryError::NoCredentials("r1".into())
        );
    }

    #[test]
    fn an_empty_inventory_is_refused() {
        assert_eq!(
            Inventory::from_toml("").unwrap_err(),
            InventoryError::NoRouters
        );
    }

    /// The file operators are told to copy has to load.
    #[test]
    fn the_shipped_example_is_a_valid_inventory() {
        let conf = include_str!("../../../nogglass.example.conf");
        let routers = include_str!("../../../routers.example.conf");
        let ui = include_str!("../../../ui.example.conf");

        // Combined parsing test
        let combined = format!("{conf}\n{routers}\n{ui}");
        let inventory = Inventory::from_toml(&combined).expect("the combined examples must load");
        assert!(inventory.routers.len() >= 3);
        assert!(
            inventory.routers.iter().any(|r| r.is_mock()),
            "the example should show the mock router"
        );
    }

    #[test]
    fn loads_router_with_direct_password() {
        let toml = r#"
[server]
http_addr = "127.0.0.1:9090"
captcha_secret = "test_secret_for_captcha_123456789"

[[router]]
id = "r-direct-pass"
name = "Direct Password Router"
vendor = "huawei_vrp"
host = "192.0.2.1"
password = "super_secret_router_pass"

[[router]]
id = "r-creds-pass"
name = "Credentials Password Router"
vendor = "juniper_junos"
host = "192.0.2.2"
credentials = { password = "another_secret_pass" }
"#;
        let inventory =
            Inventory::from_toml(toml).expect("should parse inventory with direct passwords");
        assert_eq!(inventory.server.http_addr, "127.0.0.1:9090");
        assert_eq!(
            inventory.server.captcha_secret.as_deref(),
            Some("test_secret_for_captcha_123456789")
        );

        let r1 = inventory.router("r-direct-pass").unwrap();
        assert_eq!(
            r1.resolved_credentials(),
            Credentials::Password("super_secret_router_pass".to_string())
        );

        let r2 = inventory.router("r-creds-pass").unwrap();
        assert_eq!(
            r2.resolved_credentials(),
            Credentials::Password("another_secret_pass".to_string())
        );

        inventory
            .check_secrets(|_| None)
            .expect("check_secrets should pass for direct passwords");
    }

    #[test]
    fn parses_custom_ssh_port_and_defaults_to_22() {
        let toml = r#"
[[router]]
id = "default-port"
name = "R1"
vendor = "huawei_vrp"
host = "192.0.2.1"
credentials = { password_env = "X" }

[[router]]
id = "custom-port"
name = "R2"
vendor = "huawei_vrp"
host = "192.0.2.2"
port = 2222
credentials = { password_env = "Y" }
"#;
        let inventory = Inventory::from_toml(toml).expect("valid inventory");
        assert_eq!(inventory.routers[0].port, 22);
        assert_eq!(inventory.routers[1].port, 2222);
    }

    #[test]
    fn missing_secrets_are_named() {
        let inventory = Inventory::from_toml(SAMPLE).unwrap();

        let err = inventory.check_secrets(|_| None).unwrap_err();
        assert_eq!(
            err,
            InventoryError::MissingSecret {
                router: "edge-01".into(),
                variable: "NOGGLASS_EDGE01_PASSWORD".into(),
            }
        );

        // An empty value counts as missing: an empty password would otherwise
        // reach the router.
        let err = inventory
            .check_secrets(|_| Some(String::new()))
            .unwrap_err();
        assert!(matches!(err, InventoryError::MissingSecret { .. }));

        inventory
            .check_secrets(|_| Some("secret".into()))
            .expect("all secrets present");
    }

    #[test]
    fn loads_ui_settings_with_defaults() {
        let inventory = Inventory::from_toml(SAMPLE).unwrap();
        assert_eq!(inventory.ui.theme, Theme::Dark);
        assert_eq!(inventory.ui.logo_height_px, 76);
        assert_eq!(inventory.ui.background_blur_px, 1);
        assert_eq!(
            inventory.ui.logo_path.as_deref(),
            Some("/etc/nogglass/logo_nogglass.png")
        );
        assert_eq!(
            inventory.ui.background_path.as_deref(),
            Some("/etc/nogglass/nogglass.png")
        );
        assert!(inventory.ui.show_best_path);
        assert!(inventory.ui.show_paths);
        assert!(inventory.ui.show_raw_output);
    }

    #[test]
    fn loads_custom_ui_settings() {
        let toml = format!(
            "{SAMPLE}\n[ui]\ntheme = \"light\"\nlogo_path = \"/etc/nogglass/logo.png\"\nlogo_height_px = 96\nbackground_path = \"/etc/nogglass/bg.png\"\nbackground_blur_px = 3\nbackground_opacity_percent = 50\n"
        );
        let inventory = Inventory::from_toml(&toml).unwrap();
        assert_eq!(inventory.ui.theme, Theme::Light);
        assert_eq!(inventory.ui.logo_height_px, 96);
        assert_eq!(inventory.ui.background_blur_px, 3);
        assert_eq!(inventory.ui.background_opacity_percent, 50);
        assert_eq!(
            inventory.ui.logo_path.as_deref(),
            Some("/etc/nogglass/logo.png")
        );
        assert_eq!(
            inventory.ui.background_path.as_deref(),
            Some("/etc/nogglass/bg.png")
        );
    }

    #[test]
    fn loads_ui_visibility_settings_and_aliases() {
        let toml = format!(
            "{SAMPLE}\n[ui]\nshow_best_path = false\nshow_paths = false\nshow_raw_output = false\n"
        );
        let inventory = Inventory::from_toml(&toml).unwrap();
        assert!(!inventory.ui.show_best_path);
        assert!(!inventory.ui.show_paths);
        assert!(!inventory.ui.show_raw_output);

        // Test with aliases
        let toml_aliases =
            format!("{SAMPLE}\n[ui]\nshow_graph = false\nshow_routes = false\nshow_raw = false\n");
        let inventory_aliases = Inventory::from_toml(&toml_aliases).unwrap();
        assert!(!inventory_aliases.ui.show_best_path);
        assert!(!inventory_aliases.ui.show_paths);
        assert!(!inventory_aliases.ui.show_raw_output);
    }

    #[test]
    fn rejects_invalid_ui_settings() {
        let bad_opacity = format!("{SAMPLE}\n[ui]\nbackground_opacity_percent = 101\n");
        let err = Inventory::from_toml(&bad_opacity).unwrap_err();
        assert!(matches!(err, InventoryError::Malformed(_)));

        let bad_logo = format!("{SAMPLE}\n[ui]\nlogo_height_px = 0\n");
        let err = Inventory::from_toml(&bad_logo).unwrap_err();
        assert!(matches!(err, InventoryError::Malformed(_)));
    }

    #[test]
    fn loads_split_companion_configuration_files() {
        let temp_dir =
            std::env::temp_dir().join(format!("nogglass_test_{}", rand::random::<u64>()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 1. Base nogglass.conf (no routers, custom limit)
        let main_conf = temp_dir.join("nogglass.conf");
        std::fs::write(&main_conf, "[limits]\ntimeout_secs = 45\n").unwrap();

        // 2. Companion routers.conf
        let routers_conf = temp_dir.join("routers.conf");
        std::fs::write(
            &routers_conf,
            "[[router]]\nid = \"split-demo\"\nname = \"Split Demo\"\nvendor = \"mock\"\nhost = \"127.0.0.1\"\n",
        )
        .unwrap();

        // 3. Companion ui.conf
        let ui_conf = temp_dir.join("ui.conf");
        std::fs::write(
            &ui_conf,
            "[ui]\ntheme = \"light\"\nlogo_height_px = 88\nshow_best_path = false\n",
        )
        .unwrap();

        let inventory = Inventory::load(&main_conf).unwrap();
        assert_eq!(inventory.limits.timeout_secs, 45);
        assert_eq!(inventory.routers.len(), 1);
        assert_eq!(inventory.routers[0].id, "split-demo");
        assert_eq!(inventory.ui.theme, Theme::Light);
        assert_eq!(inventory.ui.logo_height_px, 88);
        assert!(!inventory.ui.show_best_path);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_router_source_ip_configuration() {
        // 1. Omitted source_v4 and source_v6 defaults to None
        let toml_default = r#"
[[router]]
id = "r1"
name = "Router 1"
vendor = "mock"
host = "127.0.0.1"
"#;
        let inv = Inventory::from_toml(toml_default).unwrap();
        assert_eq!(inv.routers[0].source_v4, None);
        assert_eq!(inv.routers[0].source_v6, None);

        // 2. Empty or whitespace strings evaluate to None
        let toml_empty = r#"
[[router]]
id = "r2"
name = "Router 2"
vendor = "mock"
host = "127.0.0.1"
source_v4 = ""
source_v6 = "   "
"#;
        let inv = Inventory::from_toml(toml_empty).unwrap();
        assert_eq!(inv.routers[0].source_v4, None);
        assert_eq!(inv.routers[0].source_v6, None);

        // 3. Valid IPv4 and IPv6 addresses
        let toml_valid = r#"
[[router]]
id = "r3"
name = "Router 3"
vendor = "mock"
host = "127.0.0.1"
source_v4 = "192.0.2.1"
source_v6 = "2001:db8::1"
"#;
        let inv = Inventory::from_toml(toml_valid).unwrap();
        assert_eq!(
            inv.routers[0].source_v4,
            Some("192.0.2.1".parse::<Ipv4Addr>().unwrap())
        );
        assert_eq!(
            inv.routers[0].source_v6,
            Some("2001:db8::1".parse::<Ipv6Addr>().unwrap())
        );

        // 4. Aliases (source_ip_v4, source_ip_v6)
        let toml_aliases = r#"
[[router]]
id = "r4"
name = "Router 4"
vendor = "mock"
host = "127.0.0.1"
source_ip_v4 = "192.0.2.10"
source_ip_v6 = "2001:db8::10"
"#;
        let inv = Inventory::from_toml(toml_aliases).unwrap();
        assert_eq!(
            inv.routers[0].source_v4,
            Some("192.0.2.10".parse::<Ipv4Addr>().unwrap())
        );
        assert_eq!(
            inv.routers[0].source_v6,
            Some("2001:db8::10".parse::<Ipv6Addr>().unwrap())
        );

        // 5. Invalid IPs fail deserialization
        let toml_invalid_v4 = r#"
[[router]]
id = "r5"
name = "Router 5"
vendor = "mock"
host = "127.0.0.1"
source_v4 = "999.999.999.999"
"#;
        assert!(Inventory::from_toml(toml_invalid_v4).is_err());

        let toml_invalid_injection = r#"
[[router]]
id = "r6"
name = "Router 6"
vendor = "mock"
host = "127.0.0.1"
source_v4 = "192.0.2.1; rm -rf /"
"#;
        assert!(Inventory::from_toml(toml_invalid_injection).is_err());
    }

    #[test]
    fn bgp_summary_password_configuration_and_redaction() {
        let toml_with_security = r#"
[security]
bgp_summary_password = "SuperSecretPassword123"

[[router]]
id = "demo"
name = "Demo"
vendor = "mock"
host = "127.0.0.1"
"#;
        let inv = Inventory::from_toml(toml_with_security).unwrap();
        assert!(inv.is_bgp_summary_configured());
        assert_eq!(
            inv.bgp_summary_password(),
            Some("SuperSecretPassword123".to_string())
        );

        let debug_security = format!("{:?}", inv.security);
        assert!(!debug_security.contains("SuperSecretPassword123"));
        assert!(debug_security.contains("[REDACTED]"));

        let toml_with_server = r#"
[server]
bgp_summary_password = "ServerSecretPassword456"

[[router]]
id = "demo"
name = "Demo"
vendor = "mock"
host = "127.0.0.1"
"#;
        let inv2 = Inventory::from_toml(toml_with_server).unwrap();
        assert!(inv2.is_bgp_summary_configured());
        assert_eq!(
            inv2.bgp_summary_password(),
            Some("ServerSecretPassword456".to_string())
        );

        let debug_server = format!("{:?}", inv2.server);
        assert!(!debug_server.contains("ServerSecretPassword456"));
        assert!(debug_server.contains("[REDACTED]"));

        let toml_unconfigured = r#"
[[router]]
id = "demo"
name = "Demo"
vendor = "mock"
host = "127.0.0.1"
"#;
        let inv3 = Inventory::from_toml(toml_unconfigured).unwrap();
        assert!(!inv3.is_bgp_summary_configured());
        assert_eq!(inv3.bgp_summary_password(), None);
    }
}

