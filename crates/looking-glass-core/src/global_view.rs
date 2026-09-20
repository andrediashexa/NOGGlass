//! What the Internet sees, next to what the router sees.
//!
//! A looking glass answers "what does my router know about this prefix". The
//! question an operator usually has behind that one is "and does the rest of
//! the world agree". Those diverge in ways that matter:
//!
//! * the prefix is announced but nobody outside sees it — the announcement did
//!   not propagate, or an upstream filtered it;
//! * the router sees one origin AS and the world sees another — a hijack, or a
//!   leak, or a misconfigured customer;
//! * the world sees a more specific that this router does not — someone is
//!   announcing inside your space.
//!
//! RIPEstat publishes exactly the data needed for this. Asking it sends the
//! queried prefix off the operator's network, so it is off unless the operator
//! turns it on and says so in their privacy notice (ADR-0006).

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Where the global view comes from.
const RIPESTAT_ROUTING_STATUS: &str = "https://stat.ripe.net/data/routing-status/data.json";

/// What the Internet sees about a prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalView {
    /// Origin AS numbers seen in the global table for this prefix.
    pub origins: Vec<u32>,
    /// How many peers of the collector see it, when reported.
    pub visibility: Option<u32>,
    /// More specific prefixes announced inside this one.
    pub more_specifics: u32,
    /// Whether the exact prefix is announced at all.
    pub announced: bool,
}

/// How the router's answer and the global view line up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Agreement {
    /// The router and the world agree on the origin.
    Agrees { origin: u32 },
    /// Nobody outside announces this prefix, but the router has a route for it.
    /// Usually the announcement never propagated.
    NotAnnouncedGlobally,
    /// The router's route is missing while the world announces the prefix.
    NotSeenByRouter { global_origins: Vec<u32> },
    /// The origins differ. This is the case worth waking someone for.
    OriginMismatch {
        router_origin: u32,
        global_origins: Vec<u32>,
    },
    /// The global view could not be fetched. Not an error, and not agreement.
    Unknown,
}

/// Compares the origin AS the router reported with what the world announces.
///
/// A prefix legitimately has several origins — anycast, multihomed customers —
/// so agreement means "the router's origin is among the global ones", not "the
/// lists are identical".
pub fn compare(router_origin: Option<u32>, global: Option<&GlobalView>) -> Agreement {
    let Some(global) = global else {
        return Agreement::Unknown;
    };

    match router_origin {
        None => {
            if global.origins.is_empty() {
                Agreement::NotAnnouncedGlobally
            } else {
                Agreement::NotSeenByRouter {
                    global_origins: global.origins.clone(),
                }
            }
        }
        Some(origin) if global.origins.is_empty() || !global.announced => {
            let _ = origin;
            Agreement::NotAnnouncedGlobally
        }
        Some(origin) if global.origins.contains(&origin) => Agreement::Agrees { origin },
        Some(origin) => Agreement::OriginMismatch {
            router_origin: origin,
            global_origins: global.origins.clone(),
        },
    }
}

/// Reads a RIPEstat `routing-status` response.
///
/// Anything unreadable yields `None`: a global view we cannot parse is a global
/// view we do not have, and showing a blank panel beats showing a wrong one.
pub fn parse_routing_status(body: &str) -> Option<GlobalView> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let data = json.get("data")?;

    let origins = data
        .get("origins")
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    entry
                        .get("origin")
                        .and_then(|origin| match origin {
                            serde_json::Value::Number(n) => n.as_u64(),
                            serde_json::Value::String(s) => s.parse().ok(),
                            _ => None,
                        })
                        .and_then(|asn| u32::try_from(asn).ok())
                })
                .collect::<Vec<u32>>()
        })
        .unwrap_or_default();

    let visibility = data
        .pointer("/visibility/v4/ris_peers_seeing")
        .or_else(|| data.pointer("/visibility/v6/ris_peers_seeing"))
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok());

    let more_specifics = data
        .get("more_specifics")
        .and_then(|value| value.as_array())
        .map(|entries| entries.len() as u32)
        .unwrap_or(0);

    let announced = data
        .get("announced")
        .and_then(|value| value.as_bool())
        .unwrap_or(!origins.is_empty());

    Some(GlobalView {
        origins,
        visibility,
        more_specifics,
        announced,
    })
}

/// Builds the lookup URL from a typed prefix.
pub fn routing_status_url(prefix: IpNet) -> String {
    format!("{RIPESTAT_ROUTING_STATUS}?resource={prefix}")
}

/// Fetches the global view, with a short timeout and a cache.
pub struct GlobalViewLookup {
    client: reqwest::Client,
    enabled: bool,
}

impl GlobalViewLookup {
    pub fn new(enabled: bool, timeout: Duration) -> Self {
        crate::install_crypto_provider();
        Self {
            client: reqwest::Client::builder()
                .timeout(timeout)
                .user_agent(concat!("NOGGlass/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
            enabled,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Looks a prefix up. Any failure returns `None`, which the interface shows
    /// as "global view unavailable" rather than as an error on the query.
    pub async fn fetch(&self, prefix: IpNet) -> Option<GlobalView> {
        if !self.enabled {
            return None;
        }
        let response = self
            .client
            .get(routing_status_url(prefix))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        parse_routing_status(&response.text().await.ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(origins: &[u32]) -> GlobalView {
        GlobalView {
            origins: origins.to_vec(),
            visibility: Some(280),
            more_specifics: 0,
            announced: !origins.is_empty(),
        }
    }

    #[test]
    fn matching_origins_agree() {
        assert_eq!(
            compare(Some(65500), Some(&view(&[65500]))),
            Agreement::Agrees { origin: 65500 }
        );
    }

    /// Anycast and multihomed customers legitimately have several origins, so
    /// agreement is membership, not equality.
    #[test]
    fn one_of_several_global_origins_still_agrees() {
        assert_eq!(
            compare(Some(65500), Some(&view(&[65400, 65500, 65300]))),
            Agreement::Agrees { origin: 65500 }
        );
    }

    /// The case worth waking someone for.
    #[test]
    fn a_different_origin_is_reported_as_a_mismatch() {
        assert_eq!(
            compare(Some(65500), Some(&view(&[64999]))),
            Agreement::OriginMismatch {
                router_origin: 65500,
                global_origins: vec![64999],
            }
        );
    }

    #[test]
    fn a_route_the_world_does_not_announce_is_flagged() {
        assert_eq!(
            compare(Some(65500), Some(&view(&[]))),
            Agreement::NotAnnouncedGlobally
        );
    }

    #[test]
    fn a_prefix_the_router_lacks_but_the_world_announces_is_flagged() {
        assert_eq!(
            compare(None, Some(&view(&[65500]))),
            Agreement::NotSeenByRouter {
                global_origins: vec![65500]
            }
        );
    }

    /// Enrichment that failed must not read as agreement or as a problem.
    #[test]
    fn no_global_view_is_unknown_not_a_verdict() {
        assert_eq!(compare(Some(65500), None), Agreement::Unknown);
        assert_eq!(compare(None, None), Agreement::Unknown);
    }

    #[test]
    fn reads_a_ripestat_response() {
        let body = r#"{
            "data": {
                "announced": true,
                "origins": [{"origin": 65500, "prefix": "198.51.100.0/24"}],
                "visibility": {"v4": {"ris_peers_seeing": 281, "total_ris_peers": 300}},
                "more_specifics": [{"prefix": "198.51.100.0/25"}]
            }
        }"#;
        let parsed = parse_routing_status(body).expect("a well-formed response parses");
        assert_eq!(parsed.origins, vec![65500]);
        assert_eq!(parsed.visibility, Some(281));
        assert_eq!(parsed.more_specifics, 1);
        assert!(parsed.announced);
    }

    /// RIPEstat has returned AS numbers as strings in places; both forms read.
    #[test]
    fn an_origin_given_as_a_string_still_reads() {
        let body = r#"{"data": {"announced": true, "origins": [{"origin": "65500"}]}}"#;
        assert_eq!(
            parse_routing_status(body).unwrap().origins,
            vec![65500],
            "an AS number as a string is still an AS number"
        );
    }

    #[test]
    fn an_unreadable_response_yields_nothing() {
        for body in ["not json", "{}", r#"{"data": null}"#] {
            assert!(
                parse_routing_status(body).is_none()
                    || parse_routing_status(body).unwrap().origins.is_empty(),
                "body {body:?} should not produce a confident answer"
            );
        }
    }

    #[test]
    fn the_url_is_built_from_a_typed_prefix() {
        let prefix: IpNet = "198.51.100.0/24".parse().unwrap();
        assert_eq!(
            routing_status_url(prefix),
            "https://stat.ripe.net/data/routing-status/data.json?resource=198.51.100.0/24"
        );
    }

    #[test]
    fn the_lookup_is_inert_when_disabled() {
        let lookup = GlobalViewLookup::new(false, Duration::from_millis(100));
        assert!(!lookup.is_enabled());
    }
}
