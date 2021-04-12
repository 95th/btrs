use std::future::Future;
use std::time::Duration;
use tokio::time;

pub async fn timeout<T, E, F>(future: F, timeout_secs: u64) -> anyhow::Result<T>
where
    F: Future<Output = Result<T, E>>,
    anyhow::Error: From<E>,
{
    let duration = Duration::from_secs(timeout_secs);
    let output = time::timeout(duration, future).await??;
    Ok(output)
}
