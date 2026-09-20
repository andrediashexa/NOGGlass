//! The HTTP API.
//!
//! Everything a visitor can reach. The rules it enforces are thin on purpose:
//! validation lives in `looking_glass_core::target`, commands in the catalogue,
//! limits in the executor. What belongs here is the shape of the API, what the
//! browser is allowed to learn, and turning an internal error into something a
//! visitor can act on without it doubling as a map of the operator's network.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router as AxumRouter};
use looking_glass_core::driver::QueryType;
use looking_glass_core::executor::{Execution, ExecutionError, Executor, QueryOutcome};
use looking_glass_core::inventory::{Inventory, PublicRouter};
use looking_glass_core::target::{parse_target, TargetError};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::StreamExt;

/// What the handlers share.
#[derive(Clone)]
pub struct AppState {
    pub executor: Arc<Executor>,
    pub inventory: Arc<Inventory>,
    pub version: VersionInfo,
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
            version: option_env!("NOGGLASS_VERSION").unwrap_or("dev").to_string(),
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
        .route("/api/version", get(version))
        .route("/api/routers", get(routers))
        .route("/api/query", post(run_query))
        .route("/api/query/stream", get(stream_query))
        .route("/api/catalogue/{vendor}", get(vendor_commands))
        .with_state(state)
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
    },
    /// Output no parser could read yet, so the visitor still gets the answer.
    Raw { output: String, truncated: bool },
}

impl From<Execution> for QueryResponse {
    fn from(execution: Execution) -> Self {
        let outcome = match execution.outcome {
            QueryOutcome::Ping(result) => OutcomeBody::Ping { result },
            QueryOutcome::Traceroute(result) => OutcomeBody::Traceroute { result },
            QueryOutcome::BgpRoute(result) => OutcomeBody::BgpRoute { result },
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

async fn run_query(
    State(state): State<AppState>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiError> {
    let target = parse_target(&request.target)?;
    let execution = state
        .executor
        .execute(&request.router, request.query_type, &target)
        .await?;
    Ok(Json(execution.into()))
}

/// The same query, as a stream of events.
///
/// The browser gets `accepted` immediately, `running` when a slot is free, and
/// `result` or `error` at the end. Partial router output is not streamed yet:
/// the transport hands back the whole answer, and inventing progress would be
/// a lie about what is happening.
async fn stream_query(
    State(state): State<AppState>,
    Query(request): Query<QueryRequest>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let (sender, receiver) = tokio::sync::mpsc::channel::<Event>(8);

    tokio::spawn(async move {
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
            Ok::<QueryResponse, ApiError>(execution.into())
        }
        .await;

        let event = match response {
            Ok(payload) => Event::default()
                .event("result")
                .json_data(payload)
                .unwrap_or_else(|_| Event::default().event("error").data("serialisation failed")),
            Err(error) => Event::default()
                .event("error")
                .json_data(error.body())
                .unwrap_or_else(|_| Event::default().event("error").data("unknown error")),
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
        let inventory = Arc::new(Inventory::from_toml(INVENTORY).expect("test inventory"));
        let executor = Arc::new(Executor::new(
            inventory.clone(),
            Arc::new(BUILTIN.clone()),
            Arc::new(NoTransport),
        ));
        routes(AppState {
            executor,
            inventory,
            version: VersionInfo::from_build(),
        })
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
        // An unstamped build says "dev" rather than inventing a number.
        assert_eq!(body["version"], "dev");
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
        let request = Request::post("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"router":"demo","type":"bgp_route","target":"198.51.100.0/24"}"#,
            ))
            .unwrap();

        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_json(response).await;
        assert_eq!(body["kind"], "bgp_route");
        assert_eq!(body["result"]["paths"].as_array().unwrap().len(), 2);
        assert!(
            body["result"]["raw_output"].is_string(),
            "raw output is always returned"
        );
    }

    #[tokio::test]
    async fn a_hostile_target_is_refused_with_a_code_the_interface_can_translate() {
        let request = Request::post("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"router":"demo","type":"ping","target":"198.51.100.1; reload"}"#,
            ))
            .unwrap();

        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["code"], "target_unparseable");
    }

    #[tokio::test]
    async fn an_unknown_router_is_a_404() {
        let request = Request::post("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"router":"nope","type":"bgp_route","target":"198.51.100.0/24"}"#,
            ))
            .unwrap();

        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(response).await["code"], "unknown_router");
    }

    #[tokio::test]
    async fn a_query_the_router_does_not_offer_is_refused() {
        let request = Request::post("/api/query")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"router":"edge-01","type":"ping","target":"198.51.100.1"}"#,
            ))
            .unwrap();

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
}
