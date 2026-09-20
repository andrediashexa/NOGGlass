//! The web interface, embedded in the binary.
//!
//! ADR-0007: NOGGlass is one executable. The page, its stylesheet, its script
//! and the three message catalogues are compiled in, so there is no asset
//! directory to deploy, nothing to serve with a second process, and no way for
//! the interface to drift out of step with the API it talks to.
//!
//! Locale routing follows ADR-0004: `/pt`, `/en` and `/es`, with a request that
//! carries no locale redirected by `Accept-Language`.

use axum::extract::Path;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router as AxumRouter;

const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_CSS: &str = include_str!("../ui/assets/app.css");
const APP_JS: &str = include_str!("../ui/assets/app.js");
const MESSAGES_EN: &str = include_str!("../ui/messages/en.json");
const MESSAGES_PT: &str = include_str!("../ui/messages/pt-BR.json");
const MESSAGES_ES: &str = include_str!("../ui/messages/es.json");

/// Locales the interface ships in. English is the source locale.
pub const LOCALES: [&str; 3] = ["pt", "en", "es"];

pub fn routes() -> AxumRouter {
    AxumRouter::new()
        .route("/", get(redirect_to_locale))
        .route("/{locale}/", get(index))
        .route("/{locale}", get(index))
        .route("/assets/app.css", get(stylesheet))
        .route("/assets/app.js", get(script))
        .route("/assets/messages/{file}", get(messages))
}

/// Sends a visitor to a locale, honouring `Accept-Language`.
///
/// The redirect is temporary: the operator's default can change, and a browser
/// that cached a permanent redirect would keep the old language forever.
async fn redirect_to_locale(headers: HeaderMap) -> Redirect {
    let accept = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    Redirect::temporary(&format!("/{}/", negotiate(accept)))
}

/// Picks a locale from an `Accept-Language` header.
///
/// Quality values are honoured, so `pt;q=0.2, es;q=0.9` yields Spanish. An
/// unknown language falls back to the operator's default, and then to English.
pub fn negotiate(accept_language: &str) -> &'static str {
    let default = std::env::var("NOGGLASS_DEFAULT_LOCALE")
        .ok()
        .and_then(|value| LOCALES.iter().find(|l| **l == value).copied())
        .unwrap_or("en");

    let mut best: Option<(&'static str, f32)> = None;

    for part in accept_language.split(',') {
        let mut pieces = part.split(';');
        let tag = pieces.next().unwrap_or("").trim().to_ascii_lowercase();
        let quality = pieces
            .find_map(|p| p.trim().strip_prefix("q=").and_then(|q| q.parse().ok()))
            .unwrap_or(1.0_f32);

        // "pt-BR" and "pt-PT" both select Portuguese.
        let language = tag.split('-').next().unwrap_or("");
        if let Some(locale) = LOCALES.iter().find(|l| **l == language) {
            // `Option::is_none_or` would read better but needs a newer
            // compiler than the one this workspace supports.
            let better = match best {
                Some((_, best_quality)) => quality > best_quality,
                None => true,
            };
            if better {
                best = Some((locale, quality));
            }
        }
    }

    best.map(|(locale, _)| locale).unwrap_or(default)
}

async fn index(Path(locale): Path<String>) -> Response {
    let locale = locale.trim_end_matches('/');
    if !LOCALES.contains(&locale) {
        return (StatusCode::NOT_FOUND, "unknown locale").into_response();
    }

    // The locale is stamped into the document so the page renders in the right
    // language before any script runs.
    let html = INDEX_HTML
        .replace("data-locale=\"en\"", &format!("data-locale=\"{locale}\""))
        .replace("<html lang=\"en\"", &format!("<html lang=\"{locale}\""));

    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

async fn stylesheet() -> impl IntoResponse {
    asset("text/css; charset=utf-8", APP_CSS)
}

async fn script() -> impl IntoResponse {
    asset("text/javascript; charset=utf-8", APP_JS)
}

async fn messages(Path(file): Path<String>) -> Response {
    let body = match file.as_str() {
        "en.json" => MESSAGES_EN,
        "pt-BR.json" => MESSAGES_PT,
        "es.json" => MESSAGES_ES,
        _ => return (StatusCode::NOT_FOUND, "unknown catalogue").into_response(),
    };
    asset("application/json; charset=utf-8", body).into_response()
}

/// Serves an embedded asset.
///
/// Assets change only when the binary does, but the binary is replaced on
/// upgrades, so caching is short: a stale script against a new API is worse
/// than a few extra requests.
fn asset(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            ),
        ],
        body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_picks_a_supported_locale() {
        assert_eq!(negotiate("pt-BR,pt;q=0.9,en;q=0.8"), "pt");
        assert_eq!(negotiate("es-AR,es;q=0.9"), "es");
        assert_eq!(negotiate("en-GB"), "en");
    }

    #[test]
    fn quality_values_decide_when_several_match() {
        assert_eq!(negotiate("pt;q=0.2, es;q=0.9"), "es");
    }

    #[test]
    fn an_unsupported_language_falls_back() {
        // No operator default configured in tests, so English.
        assert_eq!(negotiate("ja,ko;q=0.8"), "en");
        assert_eq!(negotiate(""), "en");
    }

    /// Every key the page and the script ask for has to exist in all three
    /// catalogues, or a visitor sees a raw key.
    #[test]
    fn the_three_catalogues_carry_the_same_keys() {
        let en: serde_json::Value = serde_json::from_str(MESSAGES_EN).unwrap();
        let pt: serde_json::Value = serde_json::from_str(MESSAGES_PT).unwrap();
        let es: serde_json::Value = serde_json::from_str(MESSAGES_ES).unwrap();

        let keys = |value: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = value
                .as_object()
                .expect("a catalogue is an object")
                .keys()
                .cloned()
                .collect();
            keys.sort();
            keys
        };

        assert_eq!(keys(&en), keys(&pt));
        assert_eq!(keys(&en), keys(&es));
        assert!(!keys(&en).is_empty());
    }

    /// The page must not carry literal text: everything a visitor reads comes
    /// from a catalogue (ADR-0004).
    #[test]
    fn the_page_marks_every_visible_string_for_translation() {
        let en: serde_json::Value = serde_json::from_str(MESSAGES_EN).unwrap();
        let catalogue = en.as_object().unwrap();

        let mut referenced = 0;
        for fragment in INDEX_HTML.split("data-i18n=\"").skip(1) {
            let key = fragment.split('"').next().unwrap();
            assert!(
                catalogue.contains_key(key),
                "index.html uses {key:?}, which no catalogue defines"
            );
            referenced += 1;
        }
        assert!(referenced > 10, "the page should be fully translated");
    }
}
