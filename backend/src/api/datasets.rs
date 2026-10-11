//! Datasets HTTP handlers. Domain operations are delegated to Store.
use crate::Store;
use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::Value;
use std::sync::Arc;

use super::{blocking, ApiResult};

pub(super) async fn build_dataset(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.build_dataset(&project, &dataset, &request)
        })
        .await?,
    ))
}

pub(super) async fn create_dataset(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.add_named(&project, "datasets", &request)
        })
        .await?,
    ))
}

pub(super) async fn dataset(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| store.dataset(&project, &dataset)).await?,
    ))
}

pub(super) async fn dataset_entries(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.add_dataset_entries(&project, &dataset, &request)
        })
        .await?,
    ))
}

pub(super) async fn structured_preview(
    State(store): State<Arc<Store>>,
    Path((project, recording, channel)): Path<(String, String, String)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.structured_preview(&project, &recording, &channel, &request)
        })
        .await?,
    ))
}

pub(super) async fn structured_dataset(
    State(store): State<Arc<Store>>,
    Path((project, recording, channel)): Path<(String, String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.save_structured_dataset_as(&project, &recording, &channel, &request, &actor)
        })
        .await?,
    ))
}

pub(super) async fn dataset_csv(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
) -> ApiResult<Response> {
    let bytes = blocking(store, move |store| store.dataset_csv(&project, &dataset)).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=dataset.csv",
            ),
        ],
        bytes,
    )
        .into_response())
}

pub(super) async fn delete_dataset(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.delete_dataset_as(&project, &dataset, &actor)
        })
        .await?,
    ))
}

pub(super) async fn dataset_features(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.dataset_features(&project, &dataset)
        })
        .await?,
    ))
}

pub(super) async fn dataset_pca(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.dataset_pca(&project, &dataset, &request)
        })
        .await?,
    ))
}
