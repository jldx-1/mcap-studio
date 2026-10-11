//! Annotations HTTP handlers. Domain operations are delegated to Store.
use crate::Store;
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use serde_json::Value;
use std::sync::Arc;

use super::{blocking, ApiResult};

pub(super) async fn patch_annotations(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.patch_annotations_as(&project, &recording, &request, &actor)
        })
        .await?,
    ))
}

pub(super) async fn annotations(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.save_annotations_as(&project, &recording, &request, &actor)
        })
        .await?,
    ))
}
