//! Run filesystem/domain operations outside the asynchronous request threads.
use super::{ApiError, ApiResult};
use crate::Store;
use std::sync::Arc;
pub(super) async fn blocking<T: Send + 'static>(
    store: Arc<Store>,
    work: impl FnOnce(&Store) -> anyhow::Result<T> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(move || work(&store))
        .await
        .map_err(|error| ApiError(anyhow::anyhow!("Worker failed: {error}")))?
        .map_err(ApiError)
}
