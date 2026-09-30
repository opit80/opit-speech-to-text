//! One retry for transient provider failures.

use std::time::Duration;

use super::ProviderError;

pub const RETRY_DELAY: Duration = Duration::from_millis(500);

/// Runs `attempt`; on a retryable error waits `delay` and runs it once more.
pub async fn with_retry<T, F, Fut>(delay: Duration, mut attempt: F) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    match attempt().await {
        Err(err) if err.is_retryable() => {
            tokio::time::sleep(delay).await;
            attempt().await
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
