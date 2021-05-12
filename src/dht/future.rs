use futures::future::poll_fn;
use std::time::Duration;
use std::{
    future::Future,
    task::{Context, Poll},
};
use tokio::time;

pub async fn timeout<T, F>(future: F, timeout_secs: u64) -> anyhow::Result<T>
where
    F: Future<Output = anyhow::Result<T>>,
{
    let duration = Duration::from_secs(timeout_secs);
    time::timeout(duration, future).await?
}

/// Asynchronously call given closure only once.
/// If it resolves immediately return `Some(value)` otherwise returns `None`.
pub async fn poll_once<F, T>(mut f: F) -> Option<T>
where
    F: FnMut(&mut Context<'_>) -> Poll<T>,
{
    poll_fn(move |cx| match f(cx) {
        Poll::Ready(v) => Poll::Ready(Some(v)),
        Poll::Pending => Poll::Ready(None),
    })
    .await
}
