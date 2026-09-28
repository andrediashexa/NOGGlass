//! Running one query against one router, safely.
//!
//! The executor is where every guard meets: the query is typed
//! ([`crate::target`]), the command comes from the catalogue
//! ([`crate::catalogue`]), the router comes from the inventory
//! ([`crate::inventory`]), and this module adds what only exists at run time —
//! concurrency caps, a timeout, an output cap, and the guarantee that a session
//! is closed however the query ends.
//!
//! Transport is behind a trait. That keeps SSH out of the way of the rules, and
//! lets the rules be tested without a router: the tests here drive a fake
//! transport that can be slow, loud or broken on demand.

pub mod ssh;

use crate::catalogue::{Catalogue, CatalogueError};
use crate::driver::{
    BgpRouteResult, DriverError, PingResult, QueryTarget, QueryType, TracerouteResult, VendorDriver,
};
use crate::inventory::{Inventory, Router};
use crate::target::QueryLimits;
use crate::vendors::{
    AristaDriver, BirdDriver, CiscoDriver, DatacomDriver, FrrDriver, HuaweiVrpDriver,
    JuniperDriver, MikrotikDriver, MockDriver, NokiaSrosDriver, MOCK_VENDOR,
};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Why a query did not produce an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    UnknownRouter(String),
    /// The operator did not offer this query on this router.
    QueryNotOffered {
        router: String,
        query: String,
    },
    /// The vendor has no command, or the driver has no parser, for this query.
    Unsupported(String),
    /// The instance or the router is already running as many queries as it
    /// allows. The visitor is asked to retry, not queued indefinitely.
    Busy {
        scope: &'static str,
    },
    /// The command did not finish inside its budget.
    TimedOut {
        seconds: u64,
    },
    Transport(String),
    Parse(String),
    /// A credential the inventory names is missing from the environment.
    MissingSecret {
        router: String,
        variable: String,
    },
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRouter(id) => write!(f, "no router named {id:?}"),
            Self::QueryNotOffered { router, query } => {
                write!(f, "{router} does not offer {query}")
            }
            Self::Unsupported(what) => write!(f, "{what}"),
            Self::Busy { scope } => write!(
                f,
                "too many queries are running right now ({scope}); try again in a moment"
            ),
            Self::TimedOut { seconds } => write!(f, "the router did not answer within {seconds}s"),
            // Transport detail is deliberately vague towards the visitor: it
            // would otherwise map the operator's management network.
            Self::Transport(_) => write!(f, "could not reach the router"),
            Self::Parse(why) => write!(f, "could not read the router's answer: {why}"),
            Self::MissingSecret { router, .. } => {
                write!(f, "{router} is not configured correctly")
            }
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<CatalogueError> for ExecutionError {
    fn from(error: CatalogueError) -> Self {
        match error {
            CatalogueError::Unsupported { .. } => Self::Unsupported(error.to_string()),
            other => Self::Unsupported(other.to_string()),
        }
    }
}

impl From<DriverError> for ExecutionError {
    fn from(error: DriverError) -> Self {
        match error {
            DriverError::Unsupported { .. } => Self::Unsupported(error.to_string()),
            DriverError::Timeout(seconds) => Self::TimedOut { seconds },
            DriverError::ConnectionFailed(why) | DriverError::IoError(why) => Self::Transport(why),
            other => Self::Parse(other.to_string()),
        }
    }
}

/// What a query produced.
#[derive(Debug, Clone)]
pub enum QueryOutcome {
    Ping(PingResult),
    Traceroute(TracerouteResult),
    BgpRoute(BgpRouteResult),
    BgpSummary(crate::driver::BgpSummaryResult),
    /// Vendor output that has no parser yet, returned as text so the visitor
    /// still gets their answer.
    Raw {
        output: String,
        truncated: bool,
    },
}

/// One finished query, with what it cost.
#[derive(Debug, Clone)]
pub struct Execution {
    pub router_id: String,
    pub command: String,
    pub outcome: QueryOutcome,
    pub duration: Duration,
}

/// What a transport has to do: run one command and come back.
///
/// Implementations MUST enforce nothing: limits are applied by the executor, so
/// a transport cannot forget them.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Runs one command and returns what the router printed.
    async fn run(
        &self,
        router: &Router,
        command: &str,
        paging_command: Option<&str>,
    ) -> Result<String, DriverError>;
}

/// Runs queries against the routers in an inventory.
pub struct Executor {
    inventory: Arc<Inventory>,
    catalogue: Arc<Catalogue>,
    transport: Arc<dyn Transport>,
    /// Fills in RPKI state the router did not report. Absent unless the
    /// operator configured a validator.
    rpki: Option<Arc<crate::rpki::Enricher>>,
    /// Instance-wide cap.
    global: Arc<Semaphore>,
    /// Per-router caps, so one busy router cannot starve the others.
    per_router: HashMap<String, Arc<Semaphore>>,
}

impl Executor {
    pub fn new(
        inventory: Arc<Inventory>,
        catalogue: Arc<Catalogue>,
        transport: Arc<dyn Transport>,
    ) -> Self {
        let per_router = inventory
            .routers
            .iter()
            .map(|router| {
                let permits = router
                    .max_concurrent
                    .unwrap_or(inventory.limits.max_concurrent_per_router)
                    .max(1);
                (router.id.clone(), Arc::new(Semaphore::new(permits)))
            })
            .collect();

        Self {
            global: Arc::new(Semaphore::new(inventory.limits.max_concurrent_total.max(1))),
            inventory,
            catalogue,
            transport,
            rpki: None,
            per_router,
        }
    }

    /// Attaches Tier 2 RPKI validation (ADR-0010).
    pub fn with_rpki(mut self, enricher: Arc<crate::rpki::Enricher>) -> Self {
        self.rpki = Some(enricher);
        self
    }

    /// Runs a query, applying every limit.
    pub async fn execute(
        &self,
        router_id: &str,
        query: QueryType,
        target: &QueryTarget,
    ) -> Result<Execution, ExecutionError> {
        let router = self
            .inventory
            .router(router_id)
            .ok_or_else(|| ExecutionError::UnknownRouter(router_id.to_string()))?;

        if !router.allows(query) {
            return Err(ExecutionError::QueryNotOffered {
                router: router.id.clone(),
                query: format!("{query:?}"),
            });
        }

        let limits = self.inventory.limits.query_limits();
        let command = self.build_command(router, query, target, &limits)?;

        // Refuse rather than queue: a visitor who waits behind a queue of
        // strangers has no idea whether anything is happening, and an unbounded
        // queue is a way to keep a router busy indefinitely.
        let _global = self
            .global
            .clone()
            .try_acquire_owned()
            .map_err(|_| ExecutionError::Busy { scope: "instance" })?;
        let _router_permit = self
            .per_router
            .get(&router.id)
            .expect("every inventory router has a semaphore")
            .clone()
            .try_acquire_owned()
            .map_err(|_| ExecutionError::Busy { scope: "router" })?;

        let started = Instant::now();
        let mut outcome = if router.is_mock() {
            self.mock_outcome(query, target, &limits)?
        } else {
            self.run_on_router(router, query, target, &command, &limits)
                .await?
        };

        // Enrichment runs after the router answered, and cannot fail the query:
        // a slow or broken validator leaves the state as not-checked.
        if let (Some(enricher), QueryOutcome::BgpRoute(result)) = (&self.rpki, &mut outcome) {
            enricher.enrich(result).await;
        }

        Ok(Execution {
            router_id: router.id.clone(),
            command,
            outcome,
            duration: started.elapsed(),
        })
    }

    fn build_command(
        &self,
        router: &Router,
        query: QueryType,
        target: &QueryTarget,
        limits: &QueryLimits,
    ) -> Result<String, ExecutionError> {
        let command = match (query, target) {
            (QueryType::Ping, QueryTarget::Ip(ip)) => {
                self.catalogue.ping(&router.vendor, *ip, limits)?
            }
            (QueryType::Traceroute, QueryTarget::Ip(ip)) => {
                self.catalogue.traceroute(&router.vendor, *ip)?
            }
            (QueryType::Ping | QueryType::Traceroute, _) => {
                // Pinging a prefix or an AS number is not a thing; say so
                // rather than picking an address out of the range.
                return Err(ExecutionError::Unsupported(
                    "ping and traceroute need a single address".to_string(),
                ));
            }
            (QueryType::BgpRoute, QueryTarget::Asn(_)) => {
                return Err(ExecutionError::Unsupported(
                    "bgp_route accepts an IP address or CIDR prefix; use bgp_aspath for AS numbers"
                        .to_string(),
                ));
            }
            (QueryType::BgpRoute, _) => self.catalogue.bgp_route(&router.vendor, target)?,
            (QueryType::BgpAspath, QueryTarget::Asn(asn)) => {
                self.catalogue.bgp_aspath(&router.vendor, *asn)?
            }
            (QueryType::BgpAspath, _) => {
                return Err(ExecutionError::Unsupported(
                    "bgp_aspath requires an AS number (e.g. AS65000 or 65000)".to_string(),
                ));
            }
            (QueryType::BgpAspathV6, QueryTarget::Asn(asn)) => {
                self.catalogue.bgp_aspath_v6(&router.vendor, *asn)?
            }
            (QueryType::BgpAspathV6, _) => {
                return Err(ExecutionError::Unsupported(
                    "bgp_aspath_v6 requires an AS number (e.g. AS65000 or 65000)".to_string(),
                ));
            }
            (QueryType::BgpSummary, _) => self.catalogue.bgp_summary(&router.vendor)?,
        };
        Ok(command)
    }

    async fn run_on_router(
        &self,
        router: &Router,
        query: QueryType,
        target: &QueryTarget,
        command: &str,
        limits: &QueryLimits,
    ) -> Result<QueryOutcome, ExecutionError> {
        let paging = self
            .catalogue
            .vendor(&router.vendor)?
            .disable_paging
            .clone();

        let budget = Duration::from_secs(limits.timeout_secs);
        let raw = tokio::time::timeout(
            budget,
            self.transport.run(router, command, paging.as_deref()),
        )
        .await
        .map_err(|_| ExecutionError::TimedOut {
            seconds: limits.timeout_secs,
        })??;

        let (raw, truncated) = limits.truncate(&raw);

        let driver = driver_for(&router.vendor).ok_or_else(|| {
            ExecutionError::Unsupported(format!("no driver for {}", router.vendor))
        })?;

        // A vendor without a parser still answers: the visitor gets the text the
        // router printed, flagged as unparsed, instead of an error page.
        let outcome = match query {
            QueryType::Ping => match driver.parse_ping(&raw) {
                Ok(result) => QueryOutcome::Ping(result),
                Err(DriverError::Unsupported { .. }) => QueryOutcome::Raw {
                    output: raw,
                    truncated,
                },
                Err(other) => return Err(other.into()),
            },
            QueryType::Traceroute => match driver.parse_traceroute(&raw) {
                Ok(mut result) => {
                    // The driver reads hops; only the executor knows what was
                    // asked about, so it fills the target in.
                    if let QueryTarget::Ip(ip) = target {
                        result.target = ip.to_string();
                    }
                    QueryOutcome::Traceroute(result)
                }
                Err(DriverError::Unsupported { .. }) => QueryOutcome::Raw {
                    output: raw,
                    truncated,
                },
                Err(other) => return Err(other.into()),
            },
            QueryType::BgpRoute | QueryType::BgpAspath | QueryType::BgpAspathV6 => {
                match driver.parse_bgp_route(&raw) {
                    Ok(mut result) => {
                        result.truncated = truncated;
                        QueryOutcome::BgpRoute(result)
                    }
                    Err(DriverError::Unsupported { .. }) => QueryOutcome::Raw {
                        output: raw,
                        truncated,
                    },
                    Err(other) => return Err(other.into()),
                }
            }
            QueryType::BgpSummary => match driver.parse_bgp_summary(&raw) {
                Ok(result) => QueryOutcome::BgpSummary(result),
                Err(DriverError::Unsupported { .. }) => QueryOutcome::Raw {
                    output: raw,
                    truncated,
                },
                Err(other) => return Err(other.into()),
            },
        };

        Ok(outcome)
    }

    fn mock_outcome(
        &self,
        query: QueryType,
        target: &QueryTarget,
        limits: &QueryLimits,
    ) -> Result<QueryOutcome, ExecutionError> {
        let mock = MockDriver;
        Ok(match (query, target) {
            (QueryType::Ping, QueryTarget::Ip(ip)) => {
                QueryOutcome::Ping(mock.ping(*ip, limits.ping_count))
            }
            (QueryType::Traceroute, QueryTarget::Ip(ip)) => {
                QueryOutcome::Traceroute(mock.traceroute(*ip))
            }
            (QueryType::Ping | QueryType::Traceroute, _) => {
                return Err(ExecutionError::Unsupported(
                    "ping and traceroute need a single address".to_string(),
                ))
            }
            (
                QueryType::BgpRoute | QueryType::BgpAspath | QueryType::BgpAspathV6,
                _,
            ) => {
                QueryOutcome::BgpRoute(mock.bgp_route(target))
            }
            (QueryType::BgpSummary, _) => QueryOutcome::BgpSummary(
                crate::summary::parse(
                    "Mock router — fabricated data\n\
                     BGP router identifier 192.0.2.1, local AS number 65001\n\
                     192.0.2.254     4        65100   12345   12300       42    0    0 05:12:33      850000\n\
                     192.0.2.253     4        65200    4321    4300       42    0    0 02:01:10          12\n\
                     192.0.2.252     4        65300       0       0        0    0    0 never    Idle\n",
                ),
            ),
        })
    }
}

/// The parser for a vendor.
pub fn driver_for(vendor: &str) -> Option<Box<dyn VendorDriver>> {
    Some(match vendor {
        "huawei_vrp" => Box::new(HuaweiVrpDriver),
        "cisco_iosxe" => Box::new(CiscoDriver::new(false)),
        "cisco_iosxr" => Box::new(CiscoDriver::new(true)),
        "juniper_junos" => Box::new(JuniperDriver),
        "nokia_sros" => Box::new(NokiaSrosDriver),
        "mikrotik_routeros" => Box::new(MikrotikDriver),
        "datacom_dmos" => Box::new(DatacomDriver),
        "bird_routing_daemon" => Box::new(BirdDriver),
        "arista_eos" => Box::new(AristaDriver),
        "frr" => Box::new(FrrDriver),
        MOCK_VENDOR => Box::new(MockDriver),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::parse_target;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A router that answers whatever the test tells it to.
    struct FakeTransport {
        output: String,
        delay: Duration,
        calls: AtomicUsize,
        fail: bool,
    }

    impl FakeTransport {
        fn answering(output: &str) -> Arc<Self> {
            Arc::new(Self {
                output: output.to_string(),
                delay: Duration::ZERO,
                calls: AtomicUsize::new(0),
                fail: false,
            })
        }

        fn slow(delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                output: String::new(),
                delay,
                calls: AtomicUsize::new(0),
                fail: false,
            })
        }

        fn broken() -> Arc<Self> {
            Arc::new(Self {
                output: String::new(),
                delay: Duration::ZERO,
                calls: AtomicUsize::new(0),
                fail: true,
            })
        }
    }

    #[async_trait::async_trait]
    impl Transport for FakeTransport {
        async fn run(
            &self,
            _router: &Router,
            _command: &str,
            _paging: Option<&str>,
        ) -> Result<String, DriverError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if self.fail {
                return Err(DriverError::ConnectionFailed(
                    "management address 192.0.2.10 refused the connection".to_string(),
                ));
            }
            Ok(self.output.clone())
        }
    }

    const INVENTORY: &str = r#"
[limits]
timeout_secs = 1
max_concurrent_per_router = 1
max_concurrent_total = 4

[[router]]
id = "edge-01"
name = "Edge 01"
vendor = "huawei_vrp"
host = "192.0.2.10"
username = "nogglass"
credentials = { password_env = "NOGGLASS_EDGE01_PASSWORD" }
queries = ["ping", "bgp_route"]

[[router]]
id = "demo"
name = "Demo"
vendor = "mock"
host = "127.0.0.1"
"#;

    fn executor(transport: Arc<dyn Transport>) -> Executor {
        let inventory = Arc::new(Inventory::from_toml(INVENTORY).expect("test inventory"));
        Executor::new(
            inventory,
            Arc::new(crate::catalogue::BUILTIN.clone()),
            transport,
        )
    }

    const VRP_TABLE: &str = "\
   Network            NextHop         MED   LocPrf  PrefVal Path/Ogn
*>  198.51.100.0/24    192.0.2.254      10      150        0 65100 65500i
";

    #[tokio::test]
    async fn runs_a_bgp_query_and_parses_it() {
        let executor = executor(FakeTransport::answering(VRP_TABLE));
        let execution = executor
            .execute(
                "edge-01",
                QueryType::BgpRoute,
                &parse_target("198.51.100.0/24").unwrap(),
            )
            .await
            .expect("query should succeed");

        assert_eq!(
            execution.command,
            "display bgp routing-table 198.51.100.0 255.255.255.0"
        );
        match execution.outcome {
            QueryOutcome::BgpRoute(result) => {
                assert_eq!(result.paths.len(), 1);
                assert!(result.best().is_some());
            }
            other => panic!("expected a BGP result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_mock_router_never_touches_the_transport() {
        let transport = FakeTransport::answering("should not be used");
        let executor = executor(transport.clone());

        let execution = executor
            .execute(
                "demo",
                QueryType::BgpRoute,
                &parse_target("198.51.100.0/24").unwrap(),
            )
            .await
            .expect("the mock answers from fixtures");

        assert!(matches!(execution.outcome, QueryOutcome::BgpRoute(_)));
        assert_eq!(
            transport.calls.load(Ordering::SeqCst),
            0,
            "the mock must not open a session"
        );
    }

    #[tokio::test]
    async fn a_query_the_operator_did_not_offer_is_refused() {
        let executor = executor(FakeTransport::answering(""));
        let error = executor
            .execute(
                "edge-01",
                QueryType::Traceroute,
                &parse_target("198.51.100.1").unwrap(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ExecutionError::QueryNotOffered { .. }));
    }

    #[tokio::test]
    async fn a_slow_router_hits_the_timeout() {
        let executor = executor(FakeTransport::slow(Duration::from_secs(5)));
        let error = executor
            .execute(
                "edge-01",
                QueryType::BgpRoute,
                &parse_target("198.51.100.0/24").unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error, ExecutionError::TimedOut { seconds: 1 });
    }

    /// The visitor is told the router could not be reached, and nothing about
    /// the operator's management network.
    #[tokio::test]
    async fn transport_failures_do_not_leak_management_details() {
        let executor = executor(FakeTransport::broken());
        let error = executor
            .execute(
                "edge-01",
                QueryType::BgpRoute,
                &parse_target("198.51.100.0/24").unwrap(),
            )
            .await
            .unwrap_err();

        let shown = error.to_string();
        assert_eq!(shown, "could not reach the router");
        assert!(!shown.contains("192.0.2.10"));
    }

    #[tokio::test]
    async fn a_second_query_to_a_busy_router_is_refused_not_queued() {
        let executor = Arc::new(executor(FakeTransport::slow(Duration::from_millis(400))));
        let first = {
            let executor = executor.clone();
            tokio::spawn(async move {
                executor
                    .execute(
                        "edge-01",
                        QueryType::BgpRoute,
                        &parse_target("198.51.100.0/24").unwrap(),
                    )
                    .await
            })
        };

        // Let the first query take the router's only permit.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let second = executor
            .execute(
                "edge-01",
                QueryType::BgpRoute,
                &parse_target("198.51.100.0/24").unwrap(),
            )
            .await;

        assert_eq!(
            second.unwrap_err(),
            ExecutionError::Busy { scope: "router" }
        );
        let _ = first.await;
    }

    #[tokio::test]
    async fn output_beyond_the_cap_is_flagged_as_truncated() {
        let mut inventory = Inventory::from_toml(INVENTORY).unwrap();
        inventory.limits.max_output_bytes = 32;
        let executor = Executor::new(
            Arc::new(inventory),
            Arc::new(crate::catalogue::BUILTIN.clone()),
            FakeTransport::answering(&"x".repeat(4096)),
        );

        let execution = executor
            .execute(
                "edge-01",
                QueryType::BgpRoute,
                &parse_target("198.51.100.0/24").unwrap(),
            )
            .await
            .unwrap();

        match execution.outcome {
            QueryOutcome::BgpRoute(result) => {
                assert!(result.truncated, "the cap was exceeded");
                assert!(result.raw_output.len() <= 32);
            }
            other => panic!("expected a BGP result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unknown_router_is_an_error() {
        let executor = executor(FakeTransport::answering(""));
        let error = executor
            .execute(
                "not-a-router",
                QueryType::BgpRoute,
                &parse_target("198.51.100.0/24").unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error, ExecutionError::UnknownRouter("not-a-router".into()));
    }

    #[tokio::test]
    async fn pinging_a_prefix_is_refused() {
        let executor = executor(FakeTransport::answering(""));
        let error = executor
            .execute(
                "edge-01",
                QueryType::Ping,
                &parse_target("198.51.100.0/24").unwrap(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ExecutionError::Unsupported(_)));
    }
}
