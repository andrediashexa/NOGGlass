//! Visual SVG CAPTCHA generator and stateless HMAC-SHA256 validator.
//!
//! Generates a visual challenge rendered directly as an SVG vector image,
//! with optical noise lines, random letter rotations, and a cryptographically
//! signed stateless token using HMAC-SHA256 (via `ring`).
//!
//! No database or Redis required: verification is completely stateless with
//! configurable TTL (default 5 minutes).

use rand::Rng;
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
pub struct CaptchaResponse {
    pub success: bool,
    pub captcha_id: String,
    pub captcha_svg: String,
}

pub struct CaptchaEngine;

impl CaptchaEngine {
    /// Generates a visual SVG CAPTCHA challenge with optical noise lines,
    /// character rotations, and an HMAC-SHA256 signed stateless token.
    pub fn generate(secret_key: &str) -> CaptchaResponse {
        let mut rng = rand::thread_rng();

        // Legible alphabet without ambiguous characters (e.g. 0/O, 1/I)
        const CHARS: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
        let code: String = (0..5)
            .map(|_| {
                let idx = rng.gen_range(0..CHARS.len());
                CHARS[idx] as char
            })
            .collect();

        let timestamp_utc = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let signature = Self::sign_payload(&code.to_uppercase(), timestamp_utc, secret_key);

        let encoded_code = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            code.to_uppercase(),
        );

        let captcha_id = format!("{}:{}:{}", encoded_code, timestamp_utc, signature);

        let colors = ["#00F0FF", "#C471ED", "#F43F5E", "#38BDF8", "#10B981"];
        let mut letters_svg = String::new();
        let x_positions = [25, 60, 95, 130, 165];

        for (i, c) in code.chars().enumerate() {
            let x = x_positions[i];
            let y = rng.gen_range(30..40);
            let rot = rng.gen_range(-20..20);
            let color = colors[i % colors.len()];
            let font_size = rng.gen_range(24..28);
            letters_svg.push_str(&format!(
                r#"<text x="{}" y="{}" fill="{}" font-size="{}" font-weight="900" font-family="'Courier New', monospace, sans-serif" transform="rotate({}, {}, {})" filter="url(#glow)">{}</text>"#,
                x, y, color, font_size, rot, x, y, c
            ));
        }

        let mut noise_lines = String::new();
        for _ in 0..4 {
            let x1 = rng.gen_range(5..40);
            let y1 = rng.gen_range(5..45);
            let x2 = rng.gen_range(160..200);
            let y2 = rng.gen_range(5..45);
            let stroke = colors[rng.gen_range(0..colors.len())];
            noise_lines.push_str(&format!(
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1.5" stroke-opacity="0.6" stroke-dasharray="4,4"/>"#,
                x1, y1, x2, y2, stroke
            ));
        }

        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="50" viewBox="0 0 200 50" style="background: rgba(10, 15, 29, 0.95); border-radius: 6px; border: 1px solid rgba(0, 240, 255, 0.3);"><defs><filter id="glow" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="1" result="blur"/><feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>{}{}</svg>"#,
            noise_lines, letters_svg
        );

        CaptchaResponse {
            success: true,
            captcha_id,
            captcha_svg: svg,
        }
    }

    /// Verifies the submitted CAPTCHA token and answer.
    pub fn verify(
        captcha_id: &str,
        user_input_code: &str,
        secret_key: &str,
        ttl_secs: u64,
    ) -> bool {
        let parts: Vec<&str> = captcha_id.split(':').collect();
        if parts.len() != 3 {
            return false;
        }

        let encoded_code = parts[0];
        let timestamp_str = parts[1];
        let provided_sig = parts[2];

        let timestamp: i64 = match timestamp_str.parse() {
            Ok(t) => t,
            Err(_) => return false,
        };

        // Check expiration
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if now - timestamp >= ttl_secs as i64 || now < timestamp - 10 {
            return false; // Expired or future timestamp
        }

        let decoded_bytes = match base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            encoded_code,
        ) {
            Ok(b) => b,
            Err(_) => return false,
        };

        let real_code = match String::from_utf8(decoded_bytes) {
            Ok(s) => s,
            Err(_) => return false,
        };

        // Validate cryptographic signature
        let expected_sig = Self::sign_payload(&real_code, timestamp, secret_key);
        if expected_sig != provided_sig {
            return false; // Tampered token
        }

        // Case-insensitive, whitespace-trimmed comparison
        let clean_user_code = user_input_code.trim().to_uppercase();
        clean_user_code == real_code
    }

    fn sign_payload(code: &str, timestamp: i64, secret_key: &str) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret_key.as_bytes());
        let msg = format!("{}:{}", code, timestamp);
        let tag = hmac::sign(&key, msg.as_bytes());
        hex::encode(tag.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_captcha_generation_and_verification() {
        let secret = "test-secret-salt-12345";
        let res = CaptchaEngine::generate(secret);
        assert!(res.success);
        assert!(res.captcha_svg.contains("<svg"));
        assert!(res.captcha_svg.contains("</svg>"));

        // Extract real code from captcha_id
        let parts: Vec<&str> = res.captcha_id.split(':').collect();
        let code_bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            parts[0],
        ).unwrap();
        let code = String::from_utf8(code_bytes).unwrap();

        // Valid code
        assert!(CaptchaEngine::verify(&res.captcha_id, &code, secret, 300));
        assert!(CaptchaEngine::verify(&res.captcha_id, &code.to_lowercase(), secret, 300));

        // Invalid code
        assert!(!CaptchaEngine::verify(&res.captcha_id, "WRONG", secret, 300));

        // Tampered signature
        let tampered_id = format!("{}:{}:fake_sig", parts[0], parts[1]);
        assert!(!CaptchaEngine::verify(&tampered_id, &code, secret, 300));

        // Expired token (simulate past timestamp)
        let past_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0) - 301;
        let past_sig = CaptchaEngine::sign_payload(&code, past_timestamp, secret);
        let expired_id = format!("{}:{}:{}", parts[0], past_timestamp, past_sig);
        assert!(!CaptchaEngine::verify(&expired_id, &code, secret, 300));
    }
}
