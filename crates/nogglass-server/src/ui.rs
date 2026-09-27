//! The web interface, embedded in the binary.
//!
//! ADR-0007: NOGGlass is one executable. The page, its stylesheet, its script
//! and the three message catalogues are compiled in, so there is no asset
//! directory to deploy, nothing to serve with a second process, and no way for
//! the interface to drift out of step with the API it talks to.
//!
//! Locale routing follows ADR-0004: `/pt`, `/en` and `/es`, with a request that
//! carries no locale redirected by `Accept-Language`.

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router as AxumRouter;
use looking_glass_core::inventory::{Theme, UiSettings};
use std::sync::Arc;

const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_CSS: &str = include_str!("../ui/assets/app.css");
const APP_JS: &str = include_str!("../ui/assets/app.js");
const NOGGLASS_PNG: &[u8] = include_bytes!("../ui/assets/nogglass.png");
const LOGO_NOGGLASS_PNG: &[u8] = include_bytes!("../ui/assets/logo_nogglass.png");
const MESSAGES_EN: &str = include_str!("../ui/messages/en.json");
const MESSAGES_PT: &str = include_str!("../ui/messages/pt-BR.json");
const MESSAGES_ES: &str = include_str!("../ui/messages/es.json");

/// Locales the interface ships in. English is the source locale.
pub const LOCALES: [&str; 3] = ["pt", "en", "es"];

/// Runtime state for UI customizations.
#[derive(Clone, Debug)]
pub struct UiState {
    pub theme: Theme,
    pub logo_height_px: u32,
    pub background_blur_px: u32,
    pub background_opacity_percent: u32,
    pub logo_bytes: Arc<Vec<u8>>,
    pub logo_content_type: HeaderValue,
    pub bg_bytes: Arc<Vec<u8>>,
    pub bg_content_type: HeaderValue,
}

impl UiState {
    pub fn from_settings(settings: &UiSettings) -> Result<Self, String> {
        let (logo_bytes, logo_content_type) = if let Some(path) = &settings.logo_path {
            match std::fs::read(path) {
                Ok(data) => {
                    let mime = detect_mime(path, &data);
                    tracing::info!(
                        path = %path,
                        bytes = data.len(),
                        mime = ?mime.to_str().unwrap_or(""),
                        "loaded custom logo"
                    );
                    (Arc::new(data), mime)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tracing::info!(
                        path = %path,
                        "custom logo file not found; falling back to built-in logo"
                    );
                    (
                        Arc::new(LOGO_NOGGLASS_PNG.to_vec()),
                        HeaderValue::from_static("image/png"),
                    )
                }
                Err(e) => return Err(format!("cannot read logo file '{path}': {e}")),
            }
        } else {
            (
                Arc::new(LOGO_NOGGLASS_PNG.to_vec()),
                HeaderValue::from_static("image/png"),
            )
        };

        let (bg_bytes, bg_content_type) = if let Some(path) = &settings.background_path {
            match std::fs::read(path) {
                Ok(data) => {
                    let mime = detect_mime(path, &data);
                    tracing::info!(
                        path = %path,
                        bytes = data.len(),
                        mime = ?mime.to_str().unwrap_or(""),
                        "loaded custom background"
                    );
                    (Arc::new(data), mime)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tracing::info!(
                        path = %path,
                        "custom background file not found; falling back to built-in wallpaper"
                    );
                    (
                        Arc::new(NOGGLASS_PNG.to_vec()),
                        HeaderValue::from_static("image/png"),
                    )
                }
                Err(e) => return Err(format!("cannot read background file '{path}': {e}")),
            }
        } else {
            (
                Arc::new(NOGGLASS_PNG.to_vec()),
                HeaderValue::from_static("image/png"),
            )
        };

        Ok(Self {
            theme: settings.theme,
            logo_height_px: settings.logo_height_px,
            background_blur_px: settings.background_blur_px,
            background_opacity_percent: settings.background_opacity_percent,
            logo_bytes,
            logo_content_type,
            bg_bytes,
            bg_content_type,
        })
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::from_settings(&UiSettings::default()).expect("default UiSettings always loads")
    }
}

fn detect_mime(path: &str, data: &[u8]) -> HeaderValue {
    let trimmed = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let lower = path.to_ascii_lowercase();
    if trimmed.starts_with(b"<svg")
        || trimmed.starts_with(b"<?xml")
        || lower.ends_with(".svg")
    {
        HeaderValue::from_static("image/svg+xml")
    } else if trimmed.starts_with(b"\xff\xd8\xff")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
    {
        HeaderValue::from_static("image/jpeg")
    } else if (trimmed.starts_with(b"RIFF") && trimmed.len() > 12 && &trimmed[8..12] == b"WEBP")
        || lower.ends_with(".webp")
    {
        HeaderValue::from_static("image/webp")
    } else if trimmed.starts_with(b"GIF8") || lower.ends_with(".gif") {
        HeaderValue::from_static("image/gif")
    } else {
        HeaderValue::from_static("image/png")
    }
}

pub fn routes(state: Arc<UiState>) -> AxumRouter {
    AxumRouter::new()
        .route("/", get(redirect_to_locale))
        .route("/{locale}/", get(index))
        .route("/{locale}", get(index))
        .route("/assets/app.css", get(stylesheet))
        .route("/assets/app.js", get(script))
        .route("/assets/nogglass.png", get(bg_png))
        .route("/assets/logo_nogglass.png", get(logo_png))
        .route("/assets/messages/{file}", get(messages))
        .with_state(state)
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

async fn index(State(ui): State<Arc<UiState>>, Path(locale): Path<String>) -> Response {
    let locale = locale.trim_end_matches('/');
    if !LOCALES.contains(&locale) {
        return (StatusCode::NOT_FOUND, "unknown locale").into_response();
    }

    let theme_str = match ui.theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    };

    let custom_style = format!(
        "<style id=\"nogglass-custom-vars\">:root {{ --logo-height: {}px !important; --bg-blur: {}px !important; --bg-opacity: {:.2} !important; }}</style>",
        ui.logo_height_px,
        ui.background_blur_px,
        (ui.background_opacity_percent as f32) / 100.0
    );

    // The locale and theme are stamped into the document so the page renders in the right
    // language and theme before any script runs.
    let html = INDEX_HTML
        .replace("data-locale=\"en\"", &format!("data-locale=\"{locale}\""))
        .replace(
            "<html lang=\"en\"",
            &format!("<html lang=\"{locale}\" data-theme=\"{theme_str}\""),
        )
        .replace("<!-- NOGGLASS_CUSTOM_VARS -->", &custom_style);

    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(
                    "no-cache, no-store, must-revalidate, max-age=0, s-maxage=0",
                ),
            ),
        ],
        html,
    )
        .into_response()
}

async fn stylesheet() -> impl IntoResponse {
    asset("text/css; charset=utf-8", APP_CSS)
}

async fn script() -> impl IntoResponse {
    asset("text/javascript; charset=utf-8", APP_JS)
}

async fn bg_png(State(ui): State<Arc<UiState>>) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, ui.bg_content_type.clone()),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(
                    "no-cache, no-store, must-revalidate, max-age=0, s-maxage=0",
                ),
            ),
        ],
        (*ui.bg_bytes).clone(),
    )
}

async fn logo_png(State(ui): State<Arc<UiState>>) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, ui.logo_content_type.clone()),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(
                    "no-cache, no-store, must-revalidate, max-age=0, s-maxage=0",
                ),
            ),
        ],
        (*ui.logo_bytes).clone(),
    )
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
/// Assets change only when the binary does, but when running behind edge CDNs
/// like Cloudflare, s-maxage=0 ensures changes reflect immediately.
fn asset(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(
                    "no-cache, must-revalidate, max-age=0, s-maxage=0",
                ),
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

    #[test]
    fn ui_state_loads_default_embedded_assets() {
        let state = UiState::default();
        assert_eq!(state.theme, Theme::Dark);
        assert_eq!(state.logo_height_px, 76);
        assert_eq!(state.background_blur_px, 1);
        assert_eq!(state.background_opacity_percent, 35);
        assert_eq!(state.logo_content_type, "image/png");
        assert_eq!(state.bg_content_type, "image/png");
        assert_eq!(state.logo_bytes.as_slice(), LOGO_NOGGLASS_PNG);
        assert_eq!(state.bg_bytes.as_slice(), NOGGLASS_PNG);
    }

    #[test]
    fn ui_state_falls_back_when_file_not_found() {
        let settings = UiSettings {
            logo_path: Some("/nonexistent/custom_logo_12345.png".to_string()),
            background_path: Some("/nonexistent/custom_bg_12345.png".to_string()),
            ..Default::default()
        };
        let state = UiState::from_settings(&settings).unwrap();
        assert_eq!(state.logo_bytes.as_slice(), LOGO_NOGGLASS_PNG);
        assert_eq!(state.bg_bytes.as_slice(), NOGGLASS_PNG);
    }

    #[test]
    fn ui_state_loads_existing_custom_file() {
        let temp_dir = std::env::temp_dir();
        let custom_logo = temp_dir.join("test_custom_logo.svg");
        std::fs::write(&custom_logo, b"<svg>custom logo</svg>").unwrap();

        let settings = UiSettings {
            logo_path: Some(custom_logo.to_string_lossy().to_string()),
            ..Default::default()
        };
        let state = UiState::from_settings(&settings).unwrap();
        assert_eq!(state.logo_content_type, "image/svg+xml");
        assert_eq!(state.logo_bytes.as_slice(), b"<svg>custom logo</svg>");

        let _ = std::fs::remove_file(&custom_logo);
    }

    #[tokio::test]
    async fn index_renders_custom_theme_and_variables() {
        let settings = UiSettings {
            theme: Theme::Light,
            logo_height_px: 100,
            background_blur_px: 4,
            background_opacity_percent: 60,
            ..Default::default()
        };
        let state = Arc::new(UiState::from_settings(&settings).unwrap());
        let response = index(State(state), Path("pt".to_string())).await;
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body);

        assert!(body_str.contains("data-theme=\"light\""));
        assert!(body_str.contains("--logo-height: 100px !important"));
        assert!(body_str.contains("--bg-blur: 4px !important"));
        assert!(body_str.contains("--bg-opacity: 0.60 !important"));
    }
}
