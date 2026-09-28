//! RPKI validation, in two tiers (ADR-0010).
//!
//! **Tier 1** is whatever the router itself reported. If the router has an RTR
//! session and printed a state, that state wins: it is what the router actually
//! filtered on, which is what an operator debugging their own network needs to
//! see.
//!
//! **Tier 2** fills the blanks. Many routers in a real fleet have no RTR
//! session — a regional POP, an older platform, a MikroTik — and report
//! nothing. Rather than shrug, NOGGlass asks a validator: the operator's own
//! (Routinator or another with a compatible API) when configured, RIPEstat
//! otherwise.
//!
//! Provenance travels with the state, because "my router says this prefix is
//! invalid" and "a public API says so" are different facts about an operator's
//! filtering, and an interface that blurs them misleads.
//!
//! Enrichment never decides whether a query succeeds: a validator that is slow,
//! broken or unreachable leaves the state as not-checked.

use crate::driver::{RpkiSource, RpkiStatus, RpkiValidation};
use ipnet::IpNet;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Where Tier 2 asks.
#[derive(Debug, Clone)]
pub enum Validator {
    /// A validator the operator runs, such as Routinator's
    /// `/api/v1/validity/{asn}/{prefix}`.
    Local { base_url: String },
    /// The public RIPEstat API.
    RipeStat,
    /// No Tier 2 at all: the router's word or nothing.
    Disabled,
}

impl Default for Validator {
    fn default() -> Self {
        // A default that reaches a third party would send every visitor's query
        // off the operator's network without anyone deciding to (ADR-0006).
        Self::Disabled
    }
}

/// Tier 2 settings.
#[derive(Debug, Clone)]
pub struct RpkiConfig {
    pub validator: Validator,
    /// Hard ceiling for the lookup. Past this, the state stays not-checked.
    pub timeout: Duration,
    pub cache_ttl: Duration,
    pub cache_capacity: usize,
}

impl Default for RpkiConfig {
    fn default() -> Self {
        Self {
            validator: Validator::default(),
            timeout: Duration::from_millis(3000),
            cache_ttl: Duration::from_secs(3600),
            cache_capacity: 50_000,
        }
    }
}

/// What a validator was asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Origin {
    pub prefix: IpNet,
    pub asn: u32,
}

/// Builds the URL for a validity lookup.
///
/// Both APIs take the origin AS and the prefix as path or query parameters
/// built from typed values, so nothing a visitor typed is interpolated.
pub fn validity_url(validator: &Validator, origin: Origin) -> Option<String> {
    match validator {
        Validator::Local { base_url } => Some(format!(
            "{}/api/v1/validity/AS{}/{}",
            base_url.trim_end_matches('/'),
            origin.asn,
            origin.prefix
        )),
        Validator::RipeStat => Some(format!(
            "https://stat.ripe.net/data/rpki-validation/data.json?resource=AS{}&prefix={}",
            origin.asn, origin.prefix
        )),
        Validator::Disabled => None,
    }
}

/// Reads a validity answer from either API shape.
///
/// An answer that does not parse leaves the state not-checked: a validator we
/// cannot read is a validator we do not have.
pub fn parse_validity(validator: &Validator, body: &str) -> RpkiValidation {
    let source = match validator {
        Validator::Local { .. } => RpkiSource::LocalValidator,
        Validator::RipeStat => RpkiSource::RipeStat,
        Validator::Disabled => return RpkiValidation::default(),
    };

    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return RpkiValidation::default();
    };

    // Routinator: {"validated_route":{"validity":{"state":"valid"}}}
    // RIPEstat:   {"data":{"status":"valid"}}
    let state = json
        .pointer("/validated_route/validity/state")
        .or_else(|| json.pointer("/data/status"))
        .and_then(|value| value.as_str())
        .map(str::to_ascii_lowercase);

    let status = match state.as_deref() {
        Some("valid") => RpkiStatus::Valid,
        Some("invalid") | Some("invalid_asn") | Some("invalid_length") => RpkiStatus::Invalid,
        Some("not-found") | Some("notfound") | Some("unknown") => RpkiStatus::NotFound,
        _ => return RpkiValidation::default(),
    };

    RpkiValidation { status, source }
}

/// Decides whether Tier 2 is needed.
///
/// The router's answer wins whenever it gave one, so a fleet with RTR sessions
/// never pays for an external lookup, and an operator always sees their own
/// filtering reflected.
pub fn needs_lookup(current: RpkiValidation) -> bool {
    current.status == RpkiStatus::NotChecked
}

/// A small time-to-live cache for validity answers.
///
/// Keyed by prefix and origin AS: that pair is what a ROA covers, so two
/// visitors asking about the same prefix share one lookup.
pub struct ValidityCache {
    entries: Mutex<HashMap<Origin, (RpkiValidation, Instant)>>,
    ttl: Duration,
    capacity: usize,
}

impl ValidityCache {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
            capacity,
        }
    }

    pub fn get(&self, origin: Origin) -> Option<RpkiValidation> {
        let mut entries = self.entries.lock().ok()?;
        match entries.get(&origin) {
            Some((validation, stored_at)) if stored_at.elapsed() < self.ttl => Some(*validation),
            Some(_) => {
                entries.remove(&origin);
                None
            }
            None => None,
        }
    }

    pub fn insert(&self, origin: Origin, validation: RpkiValidation) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        if entries.len() >= self.capacity {
            // Cheap eviction: drop what has expired, and if everything is fresh
            // drop one entry. A validity answer is inexpensive to fetch again,
            // so precision here is not worth a heavier structure.
            entries.retain(|_, (_, stored_at)| stored_at.elapsed() < self.ttl);
            if entries.len() >= self.capacity {
                if let Some(key) = entries.keys().next().copied() {
                    entries.remove(&key);
                }
            }
        }
        entries.insert(origin, (validation, Instant::now()));
    }
}

/// Fills in RPKI state the router did not report.
///
/// Owns the cache and the HTTP client. One instance is shared by the whole
/// process, so two visitors asking about the same prefix share a lookup.
pub struct Enricher {
    config: RpkiConfig,
    cache: ValidityCache,
    client: reqwest::Client,
}

impl Enricher {
    pub fn new(config: RpkiConfig) -> Self {
        crate::install_crypto_provider();

        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .user_agent(concat!("NOGGlass/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            cache: ValidityCache::new(config.cache_ttl, config.cache_capacity),
            config,
            client,
        }
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self.config.validator, Validator::Disabled)
    }

    /// Looks one origin up, using the cache first.
    ///
    /// Any failure — unreachable validator, timeout, unreadable answer —
    /// returns not-checked. Enrichment never turns a good query into a bad one.
    pub async fn lookup(&self, origin: Origin) -> RpkiValidation {
        if !self.is_enabled() {
            return RpkiValidation::default();
        }
        if let Some(cached) = self.cache.get(origin) {
            return cached;
        }
        let Some(url) = validity_url(&self.config.validator, origin) else {
            return RpkiValidation::default();
        };

        let validation = match self.client.get(&url).send().await {
            Ok(response) if response.status().is_success() => match response.text().await {
                Ok(body) => parse_validity(&self.config.validator, &body),
                Err(err) => {
                    tracing::warn!(?err, %url, "failed to read RPKI validator response body");
                    RpkiValidation::default()
                }
            },
            Ok(response) => {
                tracing::warn!(status = %response.status(), %url, "RPKI validator returned non-success HTTP status");
                RpkiValidation::default()
            }
            Err(err) => {
                tracing::warn!(?err, %url, "RPKI lookup HTTP request failed or timed out");
                RpkiValidation::default()
            }
        };

        // Only cache definitive answers (Valid, Invalid, NotFound).
        // Transient network errors or timeouts (NotChecked) must NEVER poison the cache for 1 hour.
        if validation.status != RpkiStatus::NotChecked {
            self.cache.insert(origin, validation);
        }
        validation
    }

    /// Fills in every path of a result that the router left unchecked.
    pub async fn enrich(&self, result: &mut crate::driver::BgpRouteResult) {
        if !self.is_enabled() {
            return;
        }
        for path in &mut result.paths {
            if !needs_lookup(path.rpki) {
                continue;
            }
            let (Some(prefix), Some(asn)) = (path.prefix, path.origin_as()) else {
                continue;
            };
            path.rpki = self.lookup(Origin { prefix, asn }).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Origin {
        Origin {
            prefix: "198.51.100.0/24".parse().unwrap(),
            asn: 65500,
        }
    }

    #[test]
    fn the_routers_answer_wins() {
        let from_router = RpkiValidation {
            status: RpkiStatus::Invalid,
            source: RpkiSource::Router,
        };
        assert!(!needs_lookup(from_router), "no external lookup is needed");
        assert!(needs_lookup(RpkiValidation::default()));
    }

    #[test]
    fn urls_are_built_from_typed_values() {
        assert_eq!(
            validity_url(&Validator::RipeStat, origin()).unwrap(),
            "https://stat.ripe.net/data/rpki-validation/data.json?resource=AS65500&prefix=198.51.100.0/24"
        );
        assert_eq!(
            validity_url(
                &Validator::Local {
                    base_url: "http://routinator.internal:8323/".into()
                },
                origin()
            )
            .unwrap(),
            "http://routinator.internal:8323/api/v1/validity/AS65500/198.51.100.0/24"
        );
        assert_eq!(validity_url(&Validator::Disabled, origin()), None);
    }

    #[test]
    fn reads_both_api_shapes() {
        let routinator = r#"{"validated_route":{"validity":{"state":"valid"}}}"#;
        let validation = parse_validity(
            &Validator::Local {
                base_url: "http://validator".into(),
            },
            routinator,
        );
        assert_eq!(validation.status, RpkiStatus::Valid);
        assert_eq!(validation.source, RpkiSource::LocalValidator);

        let ripestat = r#"{"data":{"status":"invalid_asn"}}"#;
        let validation = parse_validity(&Validator::RipeStat, ripestat);
        assert_eq!(validation.status, RpkiStatus::Invalid);
        assert_eq!(validation.source, RpkiSource::RipeStat);
    }

    /// A validator we cannot read is a validator we do not have.
    #[test]
    fn an_unreadable_answer_leaves_the_state_unchecked() {
        for body in ["not json at all", "{}", r#"{"data":{"status":"weird"}}"#] {
            let validation = parse_validity(&Validator::RipeStat, body);
            assert_eq!(validation.status, RpkiStatus::NotChecked, "body: {body}");
            assert_eq!(validation.source, RpkiSource::None);
        }
    }

    /// Enrichment reaching a third party by default would leak every visitor's
    /// query off the operator's network without anyone choosing it.
    #[test]
    fn tier_two_is_disabled_until_the_operator_enables_it() {
        assert!(matches!(
            RpkiConfig::default().validator,
            Validator::Disabled
        ));
    }

    #[test]
    fn the_cache_returns_fresh_entries_and_drops_stale_ones() {
        let cache = ValidityCache::new(Duration::from_millis(40), 10);
        let validation = RpkiValidation {
            status: RpkiStatus::Valid,
            source: RpkiSource::RipeStat,
        };

        assert_eq!(cache.get(origin()), None);
        cache.insert(origin(), validation);
        assert_eq!(cache.get(origin()), Some(validation));

        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(cache.get(origin()), None, "the entry expired");
    }

    #[test]
    fn the_cache_stays_within_its_capacity() {
        let cache = ValidityCache::new(Duration::from_secs(60), 4);
        for asn in 65000..65010 {
            cache.insert(
                Origin {
                    prefix: "198.51.100.0/24".parse().unwrap(),
                    asn,
                },
                RpkiValidation::default(),
            );
        }
        assert!(cache.entries.lock().unwrap().len() <= 4);
    }

    fn a_route_for(prefix: &str, origin_as: u32) -> crate::driver::BgpRouteResult {
        use crate::driver::{BgpPath, BgpRouteResult};
        BgpRouteResult::new(
            vec![BgpPath {
                prefix: Some(prefix.parse().unwrap()),
                as_path: vec![65000, origin_as],
                ..BgpPath::default()
            }],
            String::new(),
        )
    }

    #[tokio::test]
    async fn enrich_fills_a_path_from_the_cache_without_touching_the_network() {
        let enricher = Enricher::new(RpkiConfig {
            validator: Validator::RipeStat,
            ..RpkiConfig::default()
        });
        // Seed the cache so the lookup is answered locally — no HTTP call.
        enricher.cache.insert(
            origin(),
            RpkiValidation {
                status: RpkiStatus::Valid,
                source: RpkiSource::RipeStat,
            },
        );

        let mut result = a_route_for("198.51.100.0/24", 65500);
        enricher.enrich(&mut result).await;

        assert_eq!(result.paths[0].rpki.status, RpkiStatus::Valid);
        assert_eq!(result.paths[0].rpki.source, RpkiSource::RipeStat);
    }

    #[tokio::test]
    async fn a_disabled_enricher_leaves_a_path_unchecked() {
        // The default validator is Disabled; enrich must be a fail-safe no-op.
        let enricher = Enricher::new(RpkiConfig::default());
        let mut result = a_route_for("198.51.100.0/24", 65500);
        enricher.enrich(&mut result).await;
        assert_eq!(result.paths[0].rpki.status, RpkiStatus::NotChecked);
    }

    #[tokio::test]
    async fn enrich_does_not_overwrite_a_status_the_router_reported() {
        let enricher = Enricher::new(RpkiConfig {
            validator: Validator::RipeStat,
            ..RpkiConfig::default()
        });
        // A different cached answer that must NOT be applied, because the path
        // already carries the router's own verdict.
        enricher.cache.insert(
            origin(),
            RpkiValidation {
                status: RpkiStatus::Invalid,
                source: RpkiSource::RipeStat,
            },
        );

        let mut result = a_route_for("198.51.100.0/24", 65500);
        result.paths[0].rpki = RpkiValidation {
            status: RpkiStatus::Valid,
            source: RpkiSource::Router,
        };
        enricher.enrich(&mut result).await;

        assert_eq!(result.paths[0].rpki.source, RpkiSource::Router);
        assert_eq!(result.paths[0].rpki.status, RpkiStatus::Valid);
    }
}
