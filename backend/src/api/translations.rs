//! Topic translation library, previews, and application to imported recordings.
use super::{blocking, ApiResult};
use crate::Store;
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use serde_json::Value;
use std::sync::Arc;

pub(super) async fn choices(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.topic_translations(&project, &recording)).await?,
    ))
}
pub(super) async fn save(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.save_topic_translation(&project, &recording, &request, &actor)
        })
        .await?,
    ))
}
pub(super) async fn preview(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.preview_topic_translation(&project, &recording, &request)
        })
        .await?,
    ))
}
pub(super) async fn apply(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.apply_topic_translations(&project, &recording, &request)
        })
        .await?,
    ))
}
