//! Retry timing and safe progress metadata for transient provider failures.

use std::time::Duration;

use super::ProviderError;

pub const RETRY_DELAY: Duration = Duration::from_millis(500);

pub const PATIENT_DELAYS: [u64; 4] = [10, 30, 60, 120];

/// Safe progress metadata; never contains provider messages or dictation content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct RetryWait {
    pub kind: &'static str,
    pub seconds: u64,
    pub attempt: usize,
    pub total: usize,
}

pub fn retry_after(value: &str, now: std::time::SystemTime) -> Option<Duration> {
    let delay =
        value.trim().parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
            httpdate::parse_http_date(value).ok().map(|date| date.duration_since(now).unwrap_or_default())
        })?;
    Some(delay.min(Duration::from_secs(120)))
}

/// Runs `attempt`; on a retryable error waits `delay` and runs it once more.
pub async fn with_retry<T, F, Fut>(delay: Duration, mut attempt: F) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    match attempt().await {
        Err(err) if err.is_retryable() => {
            tokio::time::sleep(err.retry_after().unwrap_or(delay)).await;
            attempt().await
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_accepts_seconds_and_http_dates_and_caps_them() {
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(retry_after("30", now), Some(Duration::from_secs(30)));
        assert_eq!(retry_after("999999", now), Some(Duration::from_secs(120)));
        assert_eq!(
            retry_after(&httpdate::fmt_http_date(now + Duration::from_secs(60)), now),
            Some(Duration::from_secs(60))
        );
        assert_eq!(retry_after(&httpdate::fmt_http_date(now - Duration::from_secs(60)), now), Some(Duration::ZERO));
        assert_eq!(retry_after("bad header", now), None);
        assert_eq!(retry_after("-1", now), None);
    }

    #[test]
    fn only_transient_provider_errors_are_retryable() {
        for error in [
            ProviderError::Timeout,
            ProviderError::Network("connection".into()),
            ProviderError::RateLimited,
            ProviderError::RateLimitedAfter(Duration::from_secs(10)),
            ProviderError::Server(500),
            ProviderError::Server(599),
        ] {
            assert!(error.is_retryable());
        }
        for error in [
            ProviderError::Unauthorized(401),
            ProviderError::Unauthorized(403),
            ProviderError::PayloadTooLarge,
            ProviderError::Http { status: 400, message: String::new() },
            ProviderError::Http { status: 404, message: String::new() },
            ProviderError::BadResponse("invalid response".into()),
            ProviderError::Server(400),
        ] {
            assert!(!error.is_retryable());
        }
    }

    async fn run_script(mut script: Vec<Result<i32, ProviderError>>) -> (Result<i32, ProviderError>, usize) {
        script.reverse();
        let mut calls = 0;
        let result = with_retry(Duration::ZERO, || {
            calls += 1;
            let next = script.pop().expect("no more scripted results");
            async move { next }
        })
        .await;
        (result, calls)
    }

    #[tokio::test]
    async fn retries_once_on_transient_errors() {
        assert_eq!(run_script(vec![Err(ProviderError::Server(502)), Ok(7)]).await, (Ok(7), 2));
    }

    #[tokio::test]
    async fn gives_up_after_the_second_transient_error() {
        let (result, calls) = run_script(vec![Err(ProviderError::Timeout), Err(ProviderError::RateLimited)]).await;
        assert_eq!((result, calls), (Err(ProviderError::RateLimited), 2));
    }

    #[tokio::test]
    async fn does_not_retry_permanent_errors() {
        assert_eq!(
            run_script(vec![Err(ProviderError::Unauthorized(401))]).await,
            (Err(ProviderError::Unauthorized(401)), 1)
        );
    }
}
