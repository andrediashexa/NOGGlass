//! Rate limiting for a public endpoint that reaches production routers.
//!
//! The concurrency caps in the executor stop a burst from holding a router.
//! They do not stop one visitor from running a query every second all day,
//! which is how a looking glass becomes someone's monitoring system, or their
//! denial-of-service tool.
//!
//! Two properties matter more than the algorithm:
//!
//! * **IPv6 is counted by prefix, not by address.** A visitor with a /64 —
//!   which is what a residential connection gets — has 18 quintillion
//!   addresses. Counting per address is the same as not counting.
//! * **The client address must be trustworthy.** Behind a proxy the peer
//!   address is the proxy, so `X-Forwarded-For` has to be read; from anywhere
//!   else that header is attacker-controlled and reading it turns the limiter
//!   into a bypass. The resolver here only honours it from configured proxies.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How much a visitor may ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    /// Queries allowed in a window.
    pub max_requests: u32,
    pub window: Duration,
    /// Extra allowance for a short burst, on top of the steady rate.
    pub burst: u32,
    /// If > 0, subsequent queries within this duration from the previous query
    /// require a CAPTCHA.
    pub require_captcha_within_secs: u64,
}

impl Default for RateLimit {
    fn default() -> Self {
        Self {
            max_requests: 20,
            window: Duration::from_secs(60),
            burst: 5,
            require_captcha_within_secs: 60,
        }
    }
}

/// The result of asking for permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow {
        remaining: u32,
    },
    /// Refused unless verified with a valid CAPTCHA.
    RequireCaptcha,
    /// Refused, with how long to wait. The visitor gets this as `Retry-After`.
    Deny {
        retry_after: Duration,
    },
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    pub fn is_captcha_required(&self) -> bool {
        matches!(self, Self::RequireCaptcha)
    }
}

/// The key a counter is kept under.
///
/// IPv4 is counted per address; IPv6 per /64, because a single visitor holds an
/// entire /64 and counting per address would count nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientKey(IpAddr);

impl ClientKey {
    pub fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => Self(ip),
            IpAddr::V6(v6) => {
                let segments = v6.segments();
                Self(IpAddr::V6(Ipv6Addr::new(
                    segments[0],
                    segments[1],
                    segments[2],
                    segments[3],
                    0,
                    0,
                    0,
                    0,
                )))
            }
        }
    }
}

/// One visitor's allowance, as a token bucket.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_seen: Instant,
    /// When the last query actually executed successfully.
    last_query_at: Option<Instant>,
}

/// Counts queries per client.
///
/// A token bucket rather than a fixed window: a fixed window lets a visitor
/// spend their whole allowance in the last second of one window and again in
/// the first second of the next, which is twice the intended rate at the worst
/// possible moment.
const NUM_SHARDS: usize = 16;

pub struct RateLimiter {
    limit: RateLimit,
    shards: [Mutex<HashMap<ClientKey, Bucket>>; NUM_SHARDS],
    /// Buckets idle for longer than this are dropped, so a scan of the address
    /// space cannot grow the map without bound.
    idle_eviction: Duration,
    max_entries_per_shard: usize,
}

impl RateLimiter {
    pub fn new(limit: RateLimit) -> Self {
        Self {
            idle_eviction: limit.window * 4,
            limit,
            shards: std::array::from_fn(|_| Mutex::new(HashMap::new())),
            max_entries_per_shard: 100_000 / NUM_SHARDS,
        }
    }

    fn shard_idx(&self, key: &ClientKey) -> usize {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % NUM_SHARDS
    }

    /// Tokens added per second by the steady rate.
    fn refill_per_second(&self) -> f64 {
        let seconds = self.limit.window.as_secs_f64().max(0.001);
        f64::from(self.limit.max_requests) / seconds
    }

    fn capacity(&self) -> f64 {
        f64::from(self.limit.max_requests + self.limit.burst)
    }

    /// Asks whether one query may run, considering burst/token limits and CAPTCHA intervals.
    pub fn check(&self, key: ClientKey, captcha_verified: bool) -> Decision {
        self.check_at(key, Instant::now(), captcha_verified)
    }

    /// The same, at a caller-supplied instant, so tests do not sleep.
    pub fn check_at(&self, key: ClientKey, now: Instant, captcha_verified: bool) -> Decision {
        let idx = self.shard_idx(&key);
        let Ok(mut buckets) = self.shards[idx].lock() else {
            return Decision::Deny {
                retry_after: self.limit.window,
            };
        };

        if buckets.len() >= self.max_entries_per_shard {
            buckets.retain(|_, bucket| now.duration_since(bucket.last_seen) < self.idle_eviction);
        }

        let capacity = self.capacity();
        let refill = self.refill_per_second();

        let bucket = buckets.entry(key).or_insert(Bucket {
            tokens: capacity,
            last_seen: now,
            last_query_at: None,
        });

        let elapsed = now.duration_since(bucket.last_seen).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill).min(capacity);
        bucket.last_seen = now;

        // Check CAPTCHA requirement:
        // If require_captcha_within_secs > 0, and the visitor queried less than that duration ago,
        // require CAPTCHA unless already successfully verified in this request.
        if self.limit.require_captcha_within_secs > 0 {
            if let Some(last_query) = bucket.last_query_at {
                let interval = Duration::from_secs(self.limit.require_captcha_within_secs);
                if now.duration_since(last_query) < interval && !captcha_verified {
                    return Decision::RequireCaptcha;
                }
            }
        }

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            bucket.last_query_at = Some(now);
            Decision::Allow {
                remaining: bucket.tokens as u32,
            }
        } else {
            let missing = 1.0 - bucket.tokens;
            Decision::Deny {
                retry_after: Duration::from_secs_f64((missing / refill).max(1.0)),
            }
        }
    }

    /// Drops idle buckets across shards without blocking concurrent requests.
    pub fn evict_idle(&self) {
        let now = Instant::now();
        for shard in &self.shards {
            if let Ok(mut buckets) = shard.try_lock() {
                buckets
                    .retain(|_, bucket| now.duration_since(bucket.last_seen) < self.idle_eviction);
            }
        }
    }

    pub fn tracked_clients(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.lock().map(|b| b.len()).unwrap_or(0))
            .sum()
    }
}

/// Works out which address a request really came from.
///
/// `X-Forwarded-For` is trusted only when the connection itself came from an
/// address the operator listed as a proxy. Anywhere else it is attacker
/// controlled: a limiter keyed on an unverified header is a limiter with a
/// documented bypass.
#[derive(Debug, Clone, Default)]
pub struct ClientAddress {
    trusted_proxies: Vec<IpAddr>,
}

impl ClientAddress {
    pub fn new(trusted_proxies: Vec<IpAddr>) -> Self {
        Self { trusted_proxies }
    }

    /// Parses a comma-separated list of proxy addresses, ignoring what does not
    /// parse rather than failing: a typo in one entry MUST NOT silently trust
    /// everything, and it MUST NOT stop the service from starting either.
    pub fn from_list(list: &str) -> (Self, Vec<String>) {
        let mut proxies = Vec::new();
        let mut rejected = Vec::new();
        for entry in list.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            match entry.parse::<IpAddr>() {
                Ok(address) => proxies.push(address),
                Err(_) => rejected.push(entry.to_string()),
            }
        }
        (Self::new(proxies), rejected)
    }

    pub fn trusts_any_proxy(&self) -> bool {
        !self.trusted_proxies.is_empty()
    }

    /// The address to count this request against.
    ///
    /// `forwarded_for` is the raw header value, if the request carried one.
    pub fn resolve(&self, peer: IpAddr, forwarded_for: Option<&str>) -> IpAddr {
        if !self.trusted_proxies.contains(&peer) {
            return peer;
        }
        let Some(header) = forwarded_for else {
            return peer;
        };

        // The left-most entry is the original client. Entries appended by
        // untrusted hops can be forged, which is why this only runs when the
        // immediate peer is a configured proxy.
        header
            .split(',')
            .map(str::trim)
            .find_map(|entry| entry.parse::<IpAddr>().ok())
            .unwrap_or(peer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("test address")
    }

    #[test]
    fn a_visitor_is_allowed_up_to_the_limit_and_then_refused() {
        let limiter = RateLimiter::new(RateLimit {
            max_requests: 3,
            window: Duration::from_secs(60),
            burst: 0,
            require_captcha_within_secs: 0,
        });
        let key = ClientKey::from_ip(ip("198.51.100.10"));
        let now = Instant::now();

        for attempt in 1..=3 {
            assert!(
                limiter.check_at(key, now, false).is_allowed(),
                "attempt {attempt} should be allowed"
            );
        }

        match limiter.check_at(key, now, false) {
            Decision::Deny { retry_after } => assert!(retry_after >= Duration::from_secs(1)),
            _ => panic!("the fourth query should be refused"),
        }
    }

    #[test]
    fn the_allowance_comes_back_over_time() {
        let limiter = RateLimiter::new(RateLimit {
            max_requests: 60,
            window: Duration::from_secs(60),
            burst: 0,
            require_captcha_within_secs: 0,
        });
        let key = ClientKey::from_ip(ip("198.51.100.10"));
        let start = Instant::now();

        for _ in 0..60 {
            assert!(limiter.check_at(key, start, false).is_allowed());
        }
        assert!(!limiter.check_at(key, start, false).is_allowed());

        // One token per second at this rate.
        let later = start + Duration::from_secs(2);
        assert!(limiter.check_at(key, later, false).is_allowed());
    }

    #[test]
    fn query_within_interval_requires_captcha_unless_verified() {
        let limiter = RateLimiter::new(RateLimit {
            max_requests: 10,
            window: Duration::from_secs(60),
            burst: 0,
            require_captcha_within_secs: 60,
        });
        let key = ClientKey::from_ip(ip("198.51.100.10"));
        let now = Instant::now();

        // First query allowed
        assert!(limiter.check_at(key, now, false).is_allowed());

        // Second query 10s later without captcha is challenged
        let ten_secs_later = now + Duration::from_secs(10);
        assert!(limiter
            .check_at(key, ten_secs_later, false)
            .is_captcha_required());

        // Second query with captcha is allowed
        assert!(limiter.check_at(key, ten_secs_later, true).is_allowed());

        // Query after 60s without captcha is allowed again
        let seventy_secs_later = ten_secs_later + Duration::from_secs(65);
        assert!(limiter
            .check_at(key, seventy_secs_later, false)
            .is_allowed());
    }

    /// A residential IPv6 connection holds a whole /64. Counting per address
    /// would let one visitor run unlimited queries by changing the last hextet.
    #[test]
    fn ipv6_is_counted_per_64_not_per_address() {
        let limiter = RateLimiter::new(RateLimit {
            max_requests: 2,
            window: Duration::from_secs(60),
            burst: 0,
            require_captcha_within_secs: 0,
        });
        let now = Instant::now();

        assert!(limiter
            .check_at(ClientKey::from_ip(ip("2001:db8:1:2::1")), now, false)
            .is_allowed());
        assert!(limiter
            .check_at(ClientKey::from_ip(ip("2001:db8:1:2::2")), now, false)
            .is_allowed());
        assert!(
            !limiter
                .check_at(
                    ClientKey::from_ip(ip("2001:db8:1:2::dead:beef")),
                    now,
                    false
                )
                .is_allowed(),
            "a different address in the same /64 shares the allowance"
        );

        // A different /64 is a different visitor.
        assert!(limiter
            .check_at(ClientKey::from_ip(ip("2001:db8:1:3::1")), now, false)
            .is_allowed());
    }

    #[test]
    fn separate_visitors_do_not_share_an_allowance() {
        let limiter = RateLimiter::new(RateLimit {
            max_requests: 1,
            window: Duration::from_secs(60),
            burst: 0,
            require_captcha_within_secs: 0,
        });
        let now = Instant::now();

        assert!(limiter
            .check_at(ClientKey::from_ip(ip("198.51.100.10")), now, false)
            .is_allowed());
        assert!(limiter
            .check_at(ClientKey::from_ip(ip("198.51.100.11")), now, false)
            .is_allowed());
    }

    #[test]
    fn idle_visitors_are_forgotten() {
        let limiter = RateLimiter::new(RateLimit {
            max_requests: 5,
            window: Duration::from_millis(10),
            burst: 0,
            require_captcha_within_secs: 0,
        });
        limiter.check(ClientKey::from_ip(ip("198.51.100.10")), false);
        assert_eq!(limiter.tracked_clients(), 1);

        std::thread::sleep(Duration::from_millis(60));
        limiter.evict_idle();
        assert_eq!(limiter.tracked_clients(), 0);
    }

    /// The property that makes the limiter worth having: a forged header from a
    /// stranger changes nothing.
    #[test]
    fn a_forwarded_header_is_ignored_unless_the_peer_is_a_configured_proxy() {
        let resolver = ClientAddress::new(vec![ip("192.0.2.1")]);

        // A stranger claiming to be someone else stays themselves.
        assert_eq!(
            resolver.resolve(ip("198.51.100.50"), Some("203.0.113.9")),
            ip("198.51.100.50")
        );

        // The configured proxy is believed.
        assert_eq!(
            resolver.resolve(ip("192.0.2.1"), Some("203.0.113.9")),
            ip("203.0.113.9")
        );

        // A proxy that forwards nothing is counted as itself.
        assert_eq!(resolver.resolve(ip("192.0.2.1"), None), ip("192.0.2.1"));
    }

    #[test]
    fn the_left_most_forwarded_entry_is_the_client() {
        let resolver = ClientAddress::new(vec![ip("192.0.2.1")]);
        assert_eq!(
            resolver.resolve(ip("192.0.2.1"), Some("203.0.113.9, 192.0.2.7, 192.0.2.1")),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn with_no_proxy_configured_the_peer_always_wins() {
        let resolver = ClientAddress::default();
        assert!(!resolver.trusts_any_proxy());
        assert_eq!(
            resolver.resolve(ip("198.51.100.50"), Some("203.0.113.9")),
            ip("198.51.100.50")
        );
    }

    #[test]
    fn a_malformed_proxy_entry_is_reported_not_ignored_silently() {
        let (resolver, rejected) =
            ClientAddress::from_list("192.0.2.1, not-an-address, 2001:db8::1");
        assert_eq!(rejected, vec!["not-an-address"]);
        assert_eq!(
            resolver.resolve(ip("2001:db8::1"), Some("203.0.113.9")),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn a_garbled_forwarded_header_falls_back_to_the_peer() {
        let resolver = ClientAddress::new(vec![ip("192.0.2.1")]);
        assert_eq!(
            resolver.resolve(ip("192.0.2.1"), Some("not-an-address, also-not")),
            ip("192.0.2.1")
        );
    }
}
