//! Pacing and retries.
//!
//! nemlig.com publishes no rate limit and sends no `X-RateLimit-*` headers, so there is
//! no budget to track. What it does have is Cloudflare in front of it, which answers a
//! burst with a challenge rather than a number. The client therefore paces itself at a
//! fixed minimum interval and treats a 429 as the only authority on how long to wait.

use std::time::Duration;

use reqwest::Method;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Shortest gap between two requests from one client, unless configured otherwise.
///
/// Five a second is far below what a browser session does while a page loads, and far
/// above what any of this crate's own call paths need.
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(200);

/// How long a `Retry-After` may ask for before the client gives up instead of sleeping.
pub const DEFAULT_MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

/// How many times a failed request is retried before the error is returned.
pub const DEFAULT_MAX_RETRIES: u32 = 2;

/// Enforces a minimum gap between requests across every clone of a client.
#[derive(Debug)]
pub(crate) struct Pacer {
    min_interval: Duration,
    next: Mutex<Option<Instant>>,
}

impl Pacer {
    pub(crate) fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            next: Mutex::new(None),
        }
    }

    /// Wait until the next request may go out, and claim that slot.
    ///
    /// The lock is held across the sleep on purpose: that serialises the queue, so ten
    /// callers arriving together leave at one interval apart rather than all at once.
    pub(crate) async fn acquire(&self) {
        if self.min_interval.is_zero() {
            return;
        }
        let mut next = self.next.lock().await;
        let now = Instant::now();
        let at = match *next {
            Some(at) if at > now => {
                tokio::time::sleep_until(at).await;
                at
            }
            _ => now,
        };
        *next = Some(at + self.min_interval);
    }
}

/// How hard the client tries again after a failure it did not cause.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RetryConfig {
    pub max_retries: u32,
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            max_delay: DEFAULT_MAX_RETRY_DELAY,
        }
    }
}

impl RetryConfig {
    /// How long to wait before attempt number `attempt`, counting from one.
    ///
    /// Doubling, capped. The shift is clamped so a large `max_retries` cannot overflow
    /// its way back round to a short delay.
    pub(crate) fn backoff(&self, attempt: u32) -> Duration {
        let base = Duration::from_millis(250);
        let delay = base.saturating_mul(1u32 << attempt.min(16));
        delay.min(self.max_delay)
    }
}

/// Whether sending the same request twice is safe.
///
/// GET and HEAD only. `AddToBasket` is a POST that happens to be idempotent, since it
/// sets an absolute quantity, but that is a property of one endpoint rather than of the
/// method, and the client has no way to tell the two apart from here.
pub(crate) fn is_replayable(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD)
}

/// Read a `Retry-After` header, which nemlig.com sends as a number of seconds.
pub(crate) fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn backoff_doubles_until_it_hits_the_ceiling() {
        let retry = RetryConfig {
            max_retries: 8,
            max_delay: Duration::from_secs(2),
        };
        assert_eq!(retry.backoff(0), Duration::from_millis(250));
        assert_eq!(retry.backoff(1), Duration::from_millis(500));
        assert_eq!(retry.backoff(2), Duration::from_secs(1));
        assert_eq!(retry.backoff(3), Duration::from_secs(2));
        assert_eq!(retry.backoff(30), Duration::from_secs(2));
    }

    #[test]
    fn only_reads_are_replayed() {
        assert!(is_replayable(&Method::GET));
        assert!(!is_replayable(&Method::POST));
    }

    #[test]
    fn a_retry_after_in_seconds_is_understood() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "12".parse().expect("valid"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(12)));

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().expect("valid"),
        );
        assert_eq!(retry_after(&headers), None, "a date is not a duration");
    }

    #[tokio::test(start_paused = true)]
    async fn the_pacer_spaces_requests_out() {
        let pacer = Pacer::new(Duration::from_millis(200));
        let start = Instant::now();
        for _ in 0..3 {
            pacer.acquire().await;
        }
        assert_eq!(start.elapsed(), Duration::from_millis(400));
    }

    #[tokio::test(start_paused = true)]
    async fn a_zero_interval_never_waits() {
        let pacer = Pacer::new(Duration::ZERO);
        let start = Instant::now();
        for _ in 0..100 {
            pacer.acquire().await;
        }
        assert_eq!(start.elapsed(), Duration::ZERO);
    }
}
