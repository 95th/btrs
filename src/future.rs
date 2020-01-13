use std::future::Future;

pub(crate) async fn timeout<T, E, F>(f: F, secs: u64) -> crate::Result<T>
where
    F: Future<Output = Result<T, E>>,
    crate::Error: From<E>,
{
    use tokio::time;
    let output = time::timeout(time::Duration::from_secs(secs), f).await??;
    Ok(output)
}
