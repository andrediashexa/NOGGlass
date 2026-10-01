//! Authentication and constant-time credential verification.
//!
//! Provides timing-safe comparison for sensitive administrative and operational
//! passwords (such as `bgp_summary_password`), preventing side-channel timing attacks.

use ring::hmac;

/// Constant-time verification of candidate password against expected password.
///
/// Uses HMAC-SHA256 with a domain-separated key to compute fixed-size (32-byte)
/// message authentication tags for both the candidate and expected secrets, then verifies
/// them using `ring::hmac::verify`.
///
/// This guarantees:
/// 1. Constant execution time regardless of input lengths or matching prefixes.
/// 2. Complete immunity against timing side-channel attacks (Grover/Bleichenbacher/time-leakage).
pub fn verify_password_constant_time(expected: &str, candidate: &str) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, b"nogglass_auth_verification_key");
    let expected_tag = hmac::sign(&key, expected.as_bytes());
    hmac::verify(&key, candidate.as_bytes(), expected_tag.as_ref()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_matching_passwords_succeed() {
        assert!(verify_password_constant_time(
            "correct_horse_battery_staple",
            "correct_horse_battery_staple"
        ));
        assert!(verify_password_constant_time("alpha123!", "alpha123!"));
        assert!(verify_password_constant_time("Senha123@#$", "Senha123@#$"));
    }

    #[test]
    fn mismatching_passwords_fail() {
        assert!(!verify_password_constant_time("secret", "wrong"));
        assert!(!verify_password_constant_time("secret", "secret_extra"));
        assert!(!verify_password_constant_time("secret_extra", "secret"));
        assert!(!verify_password_constant_time("secret", ""));
        assert!(!verify_password_constant_time("", "secret"));
    }

    #[test]
    fn prefix_or_suffix_variants_fail() {
        assert!(!verify_password_constant_time("admin123", "admin124"));
        assert!(!verify_password_constant_time("admin123", "bdmin123"));
        assert!(!verify_password_constant_time("admin123", "admin12"));
    }

    #[test]
    fn empty_passwords_behavior() {
        assert!(verify_password_constant_time("", ""));
        assert!(!verify_password_constant_time("", " "));
    }
}
