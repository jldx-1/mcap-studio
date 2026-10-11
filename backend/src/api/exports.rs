//! Exports HTTP handlers. Domain operations are delegated to Store.
use crate::Store;
use axum::{
    body::Body,
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use std::sync::Arc;

use super::{blocking, ApiResult};

pub(super) async fn export(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| store.export(&project, &request)).await?,
    ))
}

pub(super) async fn export_download(
    State(store): State<Arc<Store>>,
    Path((project, export)): Path<(String, String)>,
) -> ApiResult<Response> {
    let filename = format!("attachment; filename=\"mcap-export-{export}.zip\"");
    let bytes = blocking(store, move |store| store.export_zip(&project, &export)).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (header::CONTENT_DISPOSITION, filename),
        ],
        Body::from(bytes),
    )
        .into_response())
}
