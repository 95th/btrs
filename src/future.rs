use std::future::Future;

pub(crate) async fn timeout<T, E, F>(future: F, timeout_secs: u64) -> crate::Result<T>
where
    F: Future<Output = Result<T, E>>,
    crate::Error: From<E>,
{
    use tokio::time;
    let duration = time::Duration::from_secs(timeout_secs);
    let output = time::timeout(duration, future).await??;
    Ok(output)
}
