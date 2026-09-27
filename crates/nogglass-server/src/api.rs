//! The HTTP API.
//!
//! Everything a visitor can reach. The rules it enforces are thin on purpose:
//! validation lives in `looking_glass_core::target`, commands in the catalogue,
//! limits in the executor. What belongs here is the shape of the API, what the
//! browser is allowed to learn, and turning an internal error into something a
//! visitor can act on without it doubling as a map of the operator's network.

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router as AxumRouter};
use looking_glass_core::driver::{QueryTarget, QueryType};
use looking_glass_core::executor::{Execution, ExecutionError, Executor, QueryOutcome};
use looking_glass_core::global_view::{self, Agreement, GlobalView, GlobalViewLookup};
use looking_glass_core::inventory::{Inventory, PublicRouter};
use looking_glass_core::ratelimit::{ClientAddress, ClientKey, Decision, RateLimiter};
use looking_glass_core::target::{parse_target, TargetError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_stream::StreamExt;

/// What the handlers share.
#[derive(Clone)]
pub struct AppState {
    pub executor: Arc<Executor>,
    pub inventory: Arc<Inventory>,
    pub version: VersionInfo,
    /// Absent when the operator turned rate limiting off.
    pub limiter: Option<Arc<RateLimiter>>,
    /// Compares the router answer with what the Internet announces.
    pub global_view: Arc<GlobalViewLookup>,
    /// Decides which address a request is counted against.
    pub client_address: Arc<ClientAddress>,
    /// Secret key used to sign stateless HMAC-SHA256 CAPTCHAs.
    pub captcha_secret: String,
    /// Replay protection tracking recently verified CAPTCHA tokens.
    pub used_captchas: Arc<Mutex<HashSet<String>>>,
}

impl AppState {
    /// Counts one query against the visitor, if limiting is on.
    ///
    /// If the visitor queried less than 60s ago and didn't provide a valid CAPTCHA,
    /// returns `captcha_required`.
    fn check_rate_limit(
        &self,
        peer: SocketAddr,
        forwarded_for: Option<&str>,
        captcha_verified: bool,
    ) -> Option<ApiError> {
        let limiter = self.limiter.as_ref()?;
        let client = self.client_address.resolve(peer.ip(), forwarded_for);
        match limiter.check(ClientKey::from_ip(client), captcha_verified) {
            Decision::Allow { .. } => None,
            Decision::RequireCaptcha => Some(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "captcha_required",
                message: "Queries within 60 seconds require CAPTCHA verification.".to_string(),
            }),
            Decision::Deny { retry_after } => Some(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "rate_limited",
                message: format!(
                    "too many queries from your address; try again in {} seconds",
                    retry_after.as_secs().max(1)
                ),
            }),
        }
    }
}

/// Build identity, shown in the footer and on the About page.
#[derive(Clone, Debug, Serialize)]
pub struct VersionInfo {
    /// `dev` when the build did not stamp a version, rather than a wrong number.
    pub version: String,
    pub commit: String,
    pub built_at: String,
}

impl VersionInfo {
    /// Reads what the release build stamped in, falling back to honest
    /// placeholders.
    pub fn from_build() -> Self {
        Self {
            version: match option_env!("NOGGLASS_VERSION") {
                Some(v) if !v.is_empty() && v != "dev" => v.to_string(),
                _ => env!("CARGO_PKG_VERSION").to_string(),
            },
            commit: option_env!("NOGGLASS_COMMIT")
                .unwrap_or("unknown")
                .to_string(),
            built_at: option_env!("NOGGLASS_BUILT_AT")
                .unwrap_or("unknown")
                .to_string(),
        }
    }
}

pub fn routes(state: AppState) -> AxumRouter {
    AxumRouter::new()
        .route("/api/health", get(health))
        .route("/healthz", get(health))
        .route("/api/version", get(version))
        .route("/api/captcha", get(generate_captcha))
        .route("/api/routers", get(routers))
        .route("/api/query", post(run_query))
        .route("/api/query/stream", get(stream_query).post(stream_query_post))
        .route("/api/catalogue/{vendor}", get(vendor_commands))
        .with_state(state)
}

async fn generate_captcha(State(state): State<AppState>) -> impl IntoResponse {
    let challenge = looking_glass_core::CaptchaEngine::generate(&state.captcha_secret);
    Json(challenge)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn version(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.version.clone())
}

/// What the interface needs to draw the router selector.
#[derive(Serialize)]
struct RouterListing {
    #[serde(flatten)]
    router: PublicRouter,
    /// Query types this router answers, so the interface can disable the rest
    /// instead of offering a query that will be refused.
    queries: Vec<QueryType>,
}

async fn routers(State(state): State<AppState>) -> impl IntoResponse {
    let listing: Vec<RouterListing> = state
        .inventory
        .routers
        .iter()
        .map(|router| RouterListing {
            router: router.public_view(),
            queries: if router.queries.is_empty() {
                vec![
                    QueryType::Ping,
                    QueryType::Traceroute,
                    QueryType::BgpRoute,
                    QueryType::BgpSummary,
                ]
            } else {
                router.queries.clone()
            },
        })
        .collect();

    Json(listing)
}

/// Commands a vendor would run, so an operator can see exactly what NOGGlass
/// sends to their routers without reading the source.
async fn vendor_commands(
    Path(vendor): Path<String>,
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let commands = looking_glass_core::catalogue::BUILTIN
        .vendor(&vendor)
        .map_err(|_| ApiError {
            status: StatusCode::NOT_FOUND,
            code: "unknown_vendor",
            message: format!("no vendor named {vendor}"),
        })?;

    Ok(Json(serde_json::json!({
        "vendor": vendor,
        "display_name": commands.display_name,
        "disable_paging": commands.disable_paging,
        "ping": commands.ping_v4,
        "traceroute": commands.traceroute_v4,
        "bgp_route": commands.bgp_route_v4,
        "bgp_summary": commands.bgp_summary,
    })))
}

/// A query as the browser sends it.
#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub router: String,
    #[serde(rename = "type")]
    pub query_type: QueryType,
    /// Raw text from the visitor. It is parsed here and nowhere else.
    pub target: String,
    /// ID returned by GET /api/captcha
    pub captcha_id: Option<String>,
    /// User entered CAPTCHA response
    pub captcha_code: Option<String>,
}

/// The answer.
#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub router: String,
    /// The exact command that was sent. Operators ask for this, and hiding it
    /// would make the tool harder to trust rather than safer: the command is
    /// built from a fixed template, not from what the visitor typed.
    pub command: String,
    pub duration_ms: u128,
    #[serde(flatten)]
    pub outcome: OutcomeBody,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutcomeBody {
    Ping {
        result: looking_glass_core::driver::PingResult,
    },
    Traceroute {
        result: looking_glass_core::driver::TracerouteResult,
    },
    BgpRoute {
        result: looking_glass_core::driver::BgpRouteResult,
        /// What the Internet announces for the same prefix, when the operator
        /// enabled the comparison and the lookup answered in time.
        #[serde(skip_serializing_if = "Option::is_none")]
        global: Option<GlobalView>,
        /// How the two views line up. `unknown` when there is no global view.
        agreement: Agreement,
    },
    BgpSummary {
        result: looking_glass_core::driver::BgpSummaryResult,
    },
    /// Output no parser could read yet, so the visitor still gets the answer.
    Raw { output: String, truncated: bool },
}

impl From<Execution> for QueryResponse {
    fn from(execution: Execution) -> Self {
        let outcome = match execution.outcome {
            QueryOutcome::Ping(result) => OutcomeBody::Ping { result },
            QueryOutcome::Traceroute(result) => OutcomeBody::Traceroute { result },
            QueryOutcome::BgpRoute(result) => OutcomeBody::BgpRoute {
                result,
                global: None,
                agreement: Agreement::Unknown,
            },
            QueryOutcome::BgpSummary(result) => OutcomeBody::BgpSummary { result },
            QueryOutcome::Raw { output, truncated } => OutcomeBody::Raw { output, truncated },
        };
        Self {
            router: execution.router_id,
            command: execution.command,
            duration_ms: execution.duration.as_millis(),
            outcome,
        }
    }
}

/// Adds the global view to a BGP answer, when the operator enabled it.
///
/// Runs after the router answered and never fails the query: an unreachable
/// RIPEstat leaves the comparison `unknown`, which the interface renders as
/// "global view unavailable".
async fn add_global_view(state: &AppState, target: &QueryTarget, response: &mut QueryResponse) {
    if !state.global_view.is_enabled() {
        return;
    }
    let OutcomeBody::BgpRoute {
        result,
        global,
        agreement,
    } = &mut response.outcome
    else {
        return;
    };

    // Falling back to the queried prefix matters: with no route at all there is
    // no path to take a prefix from, and "the Internet announces this and your
    // router does not see it" is exactly the verdict that case needs.
    let prefix = match result.paths.iter().find_map(|path| path.prefix) {
        Some(prefix) => prefix,
        None => match target {
            QueryTarget::Prefix(prefix) => *prefix,
            _ => return,
        },
    };
    let router_origin = result
        .best()
        .or_else(|| result.paths.first())
        .and_then(|path| path.origin_as());

    let view = state.global_view.fetch(prefix).await;
    *agreement = global_view::compare(router_origin, view.as_ref());
    *global = view;
}

fn verify_request_captcha(
    state: &AppState,
    captcha_id: Option<&str>,
    captcha_code: Option<&str>,
) -> Result<bool, ApiError> {
    match (captcha_id, captcha_code) {
        (Some(id), Some(code)) if !id.trim().is_empty() && !code.trim().is_empty() => {
            // Prevent replay attacks: check if this token was already used
            let mut used = state.used_captchas.lock().unwrap();
            if used.contains(id) {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    code: "captcha_already_used",
                    message: "CAPTCHA challenge has already been used.".to_string(),
                });
            }

            if looking_glass_core::captcha::CaptchaEngine::verify(
                id,
                code,
                &state.captcha_secret,
                300,
            ) {
                // Prune expired entries if the cache grows large
                if used.len() >= 10_000 {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    used.retain(|token| {
                        token
                            .split(':')
                            .nth(1)
                            .and_then(|t| t.parse::<i64>().ok())
                            .map(|ts| now - ts < 300)
                            .unwrap_or(false)
                    });
                }
                used.insert(id.to_string());
                Ok(true)
            } else {
                Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    code: "captcha_invalid",
                    message: "Invalid or expired CAPTCHA code.".to_string(),
                })
            }
        }
        _ => Ok(false),
    }
}

async fn run_query(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(request): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiError> {
    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok());

    let captcha_verified = verify_request_captcha(
        &state,
        request.captcha_id.as_deref(),
        request.captcha_code.as_deref(),
    )?;

    if let Some(refusal) = state.check_rate_limit(peer, forwarded, captcha_verified) {
        return Err(refusal);
    }

    let target = parse_target(&request.target)?;
    let execution = match state
        .executor
        .execute(&request.router, request.query_type, &target)
        .await
    {
        Ok(exec) => exec,
        Err(err) => {
            tracing::warn!(
                router = %request.router,
                target = %request.target,
                error = %err,
                "query execution failed"
            );
            return Err(err.into());
        }
    };

    let mut response: QueryResponse = execution.into();
    add_global_view(&state, &target, &mut response).await;
    Ok(Json(response))
}

/// The same query, as a stream of events via GET query string.
async fn stream_query(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Query(request): Query<QueryRequest>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    handle_stream_query(state, peer, headers, request).await
}

/// The same query, as a stream of events via POST body (protecting parameters and tokens from GET logs).
async fn stream_query_post(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(request): Json<QueryRequest>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    handle_stream_query(state, peer, headers, request).await
}

async fn handle_stream_query(
    state: AppState,
    peer: SocketAddr,
    headers: axum::http::HeaderMap,
    request: QueryRequest,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let (sender, receiver) = tokio::sync::mpsc::channel::<Event>(8);
    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    tokio::spawn(async move {
        let captcha_verified = match verify_request_captcha(
            &state,
            request.captcha_id.as_deref(),
            request.captcha_code.as_deref(),
        ) {
            Ok(v) => v,
            Err(e) => {
                let _ = sender
                    .send(
                        Event::default()
                            .event("error")
                            .json_data(e.body())
                            .unwrap_or_else(|_| {
                                Event::default().event("error").data("captcha_invalid")
                            }),
                    )
                    .await;
                return;
            }
        };

        if let Some(refusal) = state.check_rate_limit(peer, forwarded.as_deref(), captcha_verified) {
            let _ = sender
                .send(
                    Event::default()
                        .event("error")
                        .json_data(refusal.body())
                        .unwrap_or_else(|_| Event::default().event("error").data("rate limited")),
                )
                .await;
            return;
        }

        let _ = sender
            .send(Event::default().event("accepted").data("{}"))
            .await;

        let response = async {
            let target = parse_target(&request.target)?;
            let _ = sender
                .send(Event::default().event("running").data("{}"))
                .await;
            let execution = state
                .executor
                .execute(&request.router, request.query_type, &target)
                .await?;
            let mut response: QueryResponse = execution.into();
            add_global_view(&state, &target, &mut response).await;
            Ok::<QueryResponse, ApiError>(response)
        }
        .await;

        let event = match response {
            Ok(payload) => Event::default()
                .event("result")
                .json_data(payload)
                .unwrap_or_else(|_| Event::default().event("error").data("serialisation failed")),
            Err(error) => {
                tracing::warn!(
                    router = %request.router,
                    target = %request.target,
                    code = %error.code,
                    message = %error.message,
                    "stream query execution failed"
                );
                Event::default()
                    .event("error")
                    .json_data(error.body())
                    .unwrap_or_else(|_| Event::default().event("error").data("unknown error"))
            }
        };
        let _ = sender.send(event).await;
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(receiver).map(Ok);
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// An error the visitor can read.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    /// Stable machine-readable code, so the interface can translate the message
    /// into the visitor's language instead of showing English from the server.
    code: &'static str,
    message: String,
}

impl ApiError {
    fn body(&self) -> serde_json::Value {
        serde_json::json!({ "code": self.code, "message": self.message })
    }
}

impl From<TargetError> for ApiError {
    fn from(error: TargetError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: match error {
                TargetError::Empty => "target_empty",
                TargetError::TooLong { .. } => "target_too_long",
                TargetError::NotRoutable(_) => "target_not_routable",
                TargetError::PrefixTooShort { .. } => "prefix_too_short",
                TargetError::HostBitsSet { .. } => "host_bits_set",
                TargetError::AsnOutOfRange => "asn_out_of_range",
                TargetError::Unparseable => "target_unparseable",
            },
            message: error.to_string(),
        }
    }
}

impl From<ExecutionError> for ApiError {
    fn from(error: ExecutionError) -> Self {
        let (status, code) = match &error {
            ExecutionError::UnknownRouter(_) => (StatusCode::NOT_FOUND, "unknown_router"),
            ExecutionError::QueryNotOffered { .. } => {
                (StatusCode::BAD_REQUEST, "query_not_offered")
            }
            ExecutionError::Unsupported(_) => (StatusCode::BAD_REQUEST, "unsupported"),
            // 429 with a Retry-After is what a well-behaved client needs to
            // back off instead of hammering.
            ExecutionError::Busy { .. } => (StatusCode::TOO_MANY_REQUESTS, "busy"),
            ExecutionError::TimedOut { .. } => (StatusCode::GATEWAY_TIMEOUT, "timed_out"),
            ExecutionError::Transport(_) => (StatusCode::BAD_GATEWAY, "unreachable"),
            ExecutionError::Parse(_) => (StatusCode::BAD_GATEWAY, "unreadable"),
            ExecutionError::MissingSecret { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, "misconfigured")
            }
        };
        Self {
            status,
            code,
            // Display is deliberately vague for transport failures; see
            // ExecutionError.
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body())).into_response();
        if self.status == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert("Retry-After", "5".parse().expect("static header value"));
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use looking_glass_core::catalogue::BUILTIN;
    use looking_glass_core::driver::DriverError;
    use looking_glass_core::inventory::Router as InventoryRouter;
    use tower::ServiceExt;

    /// A transport that refuses to be used: every test here goes through the
    /// mock router, so touching the network would be a bug.
    struct NoTransport;

    #[async_trait::async_trait]
    impl looking_glass_core::executor::Transport for NoTransport {
        async fn run(
            &self,
            _router: &InventoryRouter,
            _command: &str,
            _paging: Option<&str>,
        ) -> Result<String, DriverError> {
            panic!("a test reached the transport");
        }
    }

    const INVENTORY: &str = r#"
[[router]]
id = "demo"
name = "Demo router"
vendor = "mock"
host = "192.0.2.200"
location = "Fixtures"

[[router]]
id = "edge-01"
name = "Edge 01"
vendor = "huawei_vrp"
host = "192.0.2.10"
username = "nogglass"
credentials = { password_env = "NOGGLASS_EDGE01_PASSWORD" }
queries = ["bgp_route"]
"#;

    fn app() -> AxumRouter {
        app_from(INVENTORY)
    }

    fn app_from(config: &str) -> AxumRouter {
        let inventory = Arc::new(Inventory::from_toml(config).expect("test inventory"));
        let executor = Arc::new(Executor::new(
            inventory.clone(),
            Arc::new(BUILTIN.clone()),
            Arc::new(NoTransport),
        ));
        let (client_address, _) = inventory.rate_limit.to_client_address();
        let limiter = inventory
            .rate_limit
            .enabled
            .then(|| Arc::new(RateLimiter::new(inventory.rate_limit.to_limit())));

        let global_view = Arc::new(GlobalViewLookup::new(
            inventory.global_view.enabled,
            Duration::from_millis(inventory.global_view.timeout_ms),
        ));

        routes(AppState {
            executor,
            inventory,
            version: VersionInfo::from_build(),
            limiter,
            client_address: Arc::new(client_address),
            global_view,
            captcha_secret: "test_secret".to_string(),
            used_captchas: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Requests in tests carry no peer address unless one is attached, and the
    /// rate-limited handlers need one.
    fn from_peer(request: Request<Body>, peer: &str) -> Request<Body> {
        let mut request = request;
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            peer.parse::<SocketAddr>().expect("test peer address"),
        ));
        request
    }

    fn query_request(target: &str) -> Request<Body> {
        Request::post("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"router":"demo","type":"bgp_route","target":"{target}"}}"#
            )))
            .unwrap()
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).expect("a JSON body")
    }

    #[tokio::test]
    async fn health_and_version_answer() {
        let response = app()
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app()
            .oneshot(Request::get("/api/version").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_json(response).await;
        // An unstamped build falls back to the Cargo package version.
        assert_eq!(body["version"], "1.0.0");
    }

    /// The router list is public, so it must not describe the management plane.
    #[tokio::test]
    async fn the_router_list_hides_hosts_and_credentials() {
        let response = app()
            .oneshot(Request::get("/api/routers").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_json(response).await.to_string();

        assert!(body.contains("Edge 01"));
        assert!(body.contains("\"queries\""));
        for secret in ["192.0.2.10", "nogglass", "NOGGLASS_EDGE01_PASSWORD"] {
            assert!(!body.contains(secret), "leaked {secret} in {body}");
        }
    }

    #[tokio::test]
    async fn a_query_against_the_mock_returns_a_parsed_result() {
        let request = from_peer(query_request("198.51.100.0/24"), "198.51.100.77:5000");

        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_json(response).await;
        assert_eq!(body["kind"], "bgp_route");
        assert_eq!(body["result"]["paths"].as_array().unwrap().len(), 3);
        assert!(
            body["result"]["raw_output"].is_string(),
            "raw output is always returned"
        );
    }

    #[tokio::test]
    async fn a_hostile_target_is_refused_with_a_code_the_interface_can_translate() {
        let request = from_peer(
            Request::post("/api/query")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"router":"demo","type":"ping","target":"198.51.100.1; reload"}"#,
                ))
                .unwrap(),
            "198.51.100.78:5000",
        );

        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["code"], "target_unparseable");
    }

    #[tokio::test]
    async fn an_unknown_router_is_a_404() {
        let request = from_peer(
            Request::post("/api/query")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"router":"nope","type":"bgp_route","target":"198.51.100.0/24"}"#,
                ))
                .unwrap(),
            "198.51.100.79:5000",
        );

        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(response).await["code"], "unknown_router");
    }

    #[tokio::test]
    async fn a_query_the_router_does_not_offer_is_refused() {
        let request = from_peer(
            Request::post("/api/query")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"router":"edge-01","type":"ping","target":"198.51.100.1"}"#,
                ))
                .unwrap(),
            "198.51.100.80:5000",
        );

        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["code"], "query_not_offered");
    }

    /// Operators want to know exactly what runs on their routers.
    #[tokio::test]
    async fn the_catalogue_is_visible() {
        let response = app()
            .oneshot(
                Request::get("/api/catalogue/huawei_vrp")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_json(response).await;
        assert_eq!(body["disable_paging"], "screen-length 0 temporary");
        assert_eq!(body["bgp_summary"], "display bgp peer");
    }

    const LIMITED: &str = r#"
[rate_limit]
max_requests = 2
window_secs = 60
burst = 0
require_captcha_within_secs = 0

[[router]]
id = "demo"
name = "Demo router"
vendor = "mock"
host = "192.0.2.200"
"#;

    /// With no route there is no path to take a prefix from, so the comparison
    /// used to be skipped — losing the one verdict that case needs.
    #[tokio::test]
    async fn a_prefix_the_router_does_not_know_still_gets_compared() {
        let inventory = Arc::new(Inventory::from_toml(INVENTORY).unwrap());
        let executor = Arc::new(Executor::new(
            inventory.clone(),
            Arc::new(BUILTIN.clone()),
            Arc::new(NoTransport),
        ));
        let state = AppState {
            executor,
            inventory,
            version: VersionInfo::from_build(),
            limiter: None,
            client_address: Arc::new(ClientAddress::default()),
            // Disabled, so the test makes no network call: what is asserted is
            // that the prefix is resolved, not what RIPEstat would say.
            global_view: Arc::new(GlobalViewLookup::new(false, Duration::from_millis(1))),
            captcha_secret: "test_secret".to_string(),
            used_captchas: Arc::new(Mutex::new(HashSet::new())),
        };

        let target = parse_target("203.0.113.0/24").unwrap();
        let mut response = QueryResponse {
            router: "demo".into(),
            command: "show bgp 203.0.113.0/24".into(),
            duration_ms: 0,
            outcome: OutcomeBody::BgpRoute {
                result: looking_glass_core::driver::BgpRouteResult::new(Vec::new(), "% no route"),
                global: None,
                agreement: Agreement::Unknown,
            },
        };

        add_global_view(&state, &target, &mut response).await;

        // The lookup is off, so the verdict stays unknown — but reaching it at
        // all is the fix: before, the function returned before looking.
        match response.outcome {
            OutcomeBody::BgpRoute { agreement, .. } => {
                assert_eq!(agreement, Agreement::Unknown);
            }
            other => panic!("expected a BGP outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_visitor_past_their_allowance_is_refused_with_a_retry_after() {
        let app = app_from(LIMITED);

        for attempt in 1..=2 {
            let response = app
                .clone()
                .oneshot(from_peer(
                    query_request("198.51.100.0/24"),
                    "203.0.113.5:4000",
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "attempt {attempt}");
        }

        let response = app
            .clone()
            .oneshot(from_peer(
                query_request("198.51.100.0/24"),
                "203.0.113.5:4000",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key("retry-after"));
        assert_eq!(body_json(response).await["code"], "rate_limited");

        // A different visitor is unaffected.
        let response = app
            .oneshot(from_peer(
                query_request("198.51.100.0/24"),
                "203.0.113.6:4000",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// Without this, anyone could spoof a header and get an unlimited
    /// allowance, which is worse than having no limiter at all because it looks
    /// like there is one.
    #[tokio::test]
    async fn a_forwarded_header_from_a_stranger_does_not_reset_the_allowance() {
        let app = app_from(LIMITED);

        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(from_peer(
                    query_request("198.51.100.0/24"),
                    "203.0.113.7:4000",
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let mut request = from_peer(query_request("198.51.100.0/24"), "203.0.113.7:4000");
        request.headers_mut().insert(
            "x-forwarded-for",
            "198.51.100.200".parse().expect("test header"),
        );

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "the header came from an address that is not a configured proxy"
        );
    }

    /// The page has to load even for a visitor who is being rate limited, or
    /// they cannot read the message telling them to wait.
    #[tokio::test]
    async fn listing_routers_is_never_rate_limited() {
        let app = app_from(LIMITED);
        for _ in 0..5 {
            let response = app
                .clone()
                .oneshot(from_peer(
                    Request::get("/api/routers").body(Body::empty()).unwrap(),
                    "203.0.113.8:4000",
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn second_query_within_60s_requires_captcha_and_succeeds_when_solved() {
        let app = app();

        // 1. First query: runs freely without CAPTCHA
        let response = app
            .clone()
            .oneshot(from_peer(
                query_request("198.51.100.0/24"),
                "203.0.113.88:4000",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // 2. Second query within 60s without CAPTCHA: refused with captcha_required (429)
        let response = app
            .clone()
            .oneshot(from_peer(
                query_request("198.51.100.0/24"),
                "203.0.113.88:4000",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = body_json(response).await;
        assert_eq!(body["code"], "captcha_required");

        // 3. Fetch challenge from /api/captcha
        let captcha_res = app
            .clone()
            .oneshot(Request::get("/api/captcha").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(captcha_res.status(), StatusCode::OK);
        let captcha_body = body_json(captcha_res).await;
        let captcha_id = captcha_body["captcha_id"].as_str().unwrap();
        let captcha_svg = captcha_body["captcha_svg"].as_str().unwrap();

        // Decode code from SVG challenge
        let code = looking_glass_core::CaptchaEngine::extract_code_from_svg(captcha_svg).unwrap();

        // 4. Retry with valid CAPTCHA: succeeds with 200 OK
        let valid_req = Request::post("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"router":"demo","type":"bgp_route","target":"198.51.100.0/24","captcha_id":"{}","captcha_code":"{}"}}"#,
                captcha_id, code
            )))
            .unwrap();
        let response = app
            .clone()
            .oneshot(from_peer(valid_req, "203.0.113.88:4000"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // 5. Replay attack: submitting the same captcha_id again must be rejected (400 BAD REQUEST)
        let replay_req = Request::post("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"router":"demo","type":"bgp_route","target":"198.51.100.0/24","captcha_id":"{}","captcha_code":"{}"}}"#,
                captcha_id, code
            )))
            .unwrap();
        let replay_res = app
            .clone()
            .oneshot(from_peer(replay_req, "203.0.113.88:4000"))
            .await
            .unwrap();
        assert_eq!(replay_res.status(), StatusCode::BAD_REQUEST);
        let replay_body = body_json(replay_res).await;
        assert_eq!(replay_body["code"], "captcha_already_used");
    }

    #[tokio::test]
    async fn post_query_stream_accepts_json_body() {
        let app = app();
        let req = Request::post("/api/query/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"router":"demo","type":"bgp_route","target":"198.51.100.0/24"}"#,
            ))
            .unwrap();
        let res = app
            .oneshot(from_peer(req, "203.0.113.89:4000"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );
    }
}
