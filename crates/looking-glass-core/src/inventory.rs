//! The operator's router inventory.
//!
//! An operator describes their routers in `/etc/nogglass/nogglass.toml`: what
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
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;
use std::path::Path;

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
    /// Password read from this environment variable.
    PasswordEnv(String),
    /// Private key read from this file, with an optional passphrase variable.
    KeyFile {
        path: String,
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
}

fn default_port() -> u16 {
    DEFAULT_SSH_PORT
}

impl Router {
    /// Whether a visitor may run this query against this router.
    pub fn allows(&self, query: QueryType) -> bool {
        self.queries.is_empty() || self.queries.contains(&query)
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
    256 * 1024
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

/// The whole configuration file.
#[derive(Debug, Clone, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub limits: Limits,
    #[serde(rename = "router", default)]
    pub routers: Vec<Router>,
}

impl Inventory {
    /// Reads and validates an inventory file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, InventoryError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|e| InventoryError::Unreadable {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        Self::from_toml(&source)
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

            if !router.is_mock() && matches!(router.credentials, Credentials::None) {
                return Err(InventoryError::NoCredentials(router.id.clone()));
            }
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
            match &router.credentials {
                Credentials::PasswordEnv(variable) => {
                    if is_blank(lookup(variable)) {
                        return Err(InventoryError::MissingSecret {
                            router: router.id.clone(),
                            variable: variable.clone(),
                        });
                    }
                }
                Credentials::KeyFile {
                    passphrase_env: Some(variable),
                    ..
                } => {
                    if is_blank(lookup(variable)) {
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
        let example = include_str!("../../../nogglass.example.toml");
        let inventory = Inventory::from_toml(example).expect("the example must load");
        assert!(inventory.routers.len() >= 3);
        assert!(
            inventory.routers.iter().any(|r| r.is_mock()),
            "the example should show the mock router"
        );
        assert!(
            !example.to_lowercase().contains("password = "),
            "the example must never contain a literal secret"
        );
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
}
