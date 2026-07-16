//! Bounded HTTP retry policy (Phase 3.4).
//!
//! Retry **only** connection failure, 408, 429, and 5xx — at most twice
//! (3 total attempts). Backoff is `Retry-After` when present and sane, else
//! capped exponential jitter. Never retries auth/schema/input/blocked/policy
//! errors (4xx other than 408/429).

use std::time::Duration;

/// Maximum number of *retries* after the first attempt (roadmap: twice).
pub const MAX_RETRIES: u32 = 2;
/// Base delay for exponential backoff (attempt 0 → 200ms, then 400ms).
const BASE_DELAY: Duration = Duration::from_millis(200);
/// Hard cap on any single sleep.
const MAX_DELAY: Duration = Duration::from_secs(5);

/// Classify an HTTP status for the retry policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryClass {
    /// 2xx — success, no retry.
    Success,
    /// 408 / 429 / 5xx — retriable.
    Retriable,
    /// Other 4xx / policy — terminal, do not retry.
    Terminal,
}

/// Classify a status code.
pub fn classify_status(code: u16) -> RetryClass {
    if (200..300).contains(&code) {
        RetryClass::Success
    } else if code == 408 || code == 429 || (500..600).contains(&code) {
        RetryClass::Retriable
    } else {
        RetryClass::Terminal
    }
}

/// Parse a `Retry-After` header value (seconds or HTTP-date ignored → None).
/// Caps to [`MAX_DELAY`].
pub fn parse_retry_after(raw: Option<&str>) -> Option<Duration> {
    let s = raw?.trim();
    let secs: u64 = s.parse().ok()?;
    if secs == 0 {
        return None;
    }
    Some(Duration::from_secs(secs).min(MAX_DELAY))
}

/// Delay before attempt `attempt` (0-based after a failure). Includes light
/// jitter so concurrent clients do not thundering-herd.
pub fn backoff_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(d) = retry_after {
        return d;
    }
    // 200ms * 2^attempt, capped.
    let mult = 1u64 << attempt.min(4);
    let base = BASE_DELAY.saturating_mul(mult as u32).min(MAX_DELAY);
    // ±25% jitter from a cheap LCG on the attempt counter (no extra deps).
    let jitter_span = base.as_millis() as u64 / 4;
    let j = if jitter_span == 0 {
        0
    } else {
        (attempt as u64)
            .wrapping_mul(1103515245)
            .wrapping_add(12345)
            % (jitter_span + 1)
    };
    base.saturating_sub(Duration::from_millis(jitter_span / 2)) + Duration::from_millis(j)
}

/// Run `op` up to `1 + MAX_RETRIES` times. `op` returns
/// `Ok(T)` on success, `Err((retriable, Option<retry_after_raw>, err))` on failure.
pub fn with_retries<T, E, F>(mut op: F) -> Result<T, E>
where
    F: FnMut() -> Result<T, (bool, Option<String>, E)>,
{
    let mut last: Option<E> = None;
    for attempt in 0..=MAX_RETRIES {
        match op() {
            Ok(v) => return Ok(v),
            Err((retriable, retry_after, e)) => {
                last = Some(e);
                if !retriable || attempt == MAX_RETRIES {
                    break;
                }
                let delay = backoff_delay(attempt, parse_retry_after(retry_after.as_deref()));
                std::thread::sleep(delay);
            }
        }
    }
    Err(last.expect("at least one attempt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn classifies_retriable_and_terminal() {
        assert_eq!(classify_status(200), RetryClass::Success);
        assert_eq!(classify_status(408), RetryClass::Retriable);
        assert_eq!(classify_status(429), RetryClass::Retriable);
        assert_eq!(classify_status(503), RetryClass::Retriable);
        assert_eq!(classify_status(401), RetryClass::Terminal);
        assert_eq!(classify_status(403), RetryClass::Terminal);
        assert_eq!(classify_status(404), RetryClass::Terminal);
    }
    #[test]
    fn retries_twice_then_gives_up() {
        let n = AtomicU32::new(0);
        let err: &str = with_retries(|| {
            n.fetch_add(1, Ordering::SeqCst);
            Err::<i32, _>((true, None, "fail"))
        })
        .unwrap_err();
        assert_eq!(err, "fail");
        assert_eq!(n.load(Ordering::SeqCst), MAX_RETRIES + 1);
    }

    #[test]
    fn no_retry_on_terminal() {
        let n = AtomicU32::new(0);
        let _: Result<i32, &str> = with_retries(|| {
            n.fetch_add(1, Ordering::SeqCst);
            Err((false, None, "auth"))
        });
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn succeeds_after_one_retry() {
        let n = AtomicU32::new(0);
        let v = with_retries(|| {
            let i = n.fetch_add(1, Ordering::SeqCst);
            if i == 0 {
                Err((true, None, "once"))
            } else {
                Ok(42)
            }
        })
        .unwrap();
        assert_eq!(v, 42);
        assert_eq!(n.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn parse_retry_after_caps() {
        assert_eq!(parse_retry_after(Some("2")), Some(Duration::from_secs(2)));
        assert_eq!(parse_retry_after(Some("9999")), Some(MAX_DELAY));
        assert_eq!(parse_retry_after(Some("nope")), None);
    }
}
