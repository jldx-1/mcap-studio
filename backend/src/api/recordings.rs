//! Recordings HTTP handlers. Domain operations are delegated to Store.
use crate::Store;
use anyhow::Context;
use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

use super::{blocking, ApiError, ApiResult};

pub(super) async fn recording(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            let mut value = store.recording(&project, &recording)?;
            let permissions = store
                .accounts
                .project_permissions(&actor, &store.project_folder(&project)?)?;
            if permissions.contains("annotation.view") || permissions.contains("review.view") {
                value["eligible_counts"] = store.recording_review_counts(&project, &recording)?;
            } else {
                value["annotations"]["annotations"] = serde_json::json!({});
                value["annotations"]
                    .as_object_mut()
                    .unwrap()
                    .remove("tracks");
            }
            Ok(value)
        })
        .await?,
    ))
}

#[derive(Default, serde::Deserialize)]
pub(super) struct ImportOptions {
    #[serde(default)]
    defer_index: bool,
}
pub(super) async fn import_recording(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Query(options): Query<ImportOptions>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let mut uploaded = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError(anyhow::anyhow!("Upload error: {error}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        if uploaded.is_some() {
            return Err(ApiError(anyhow::anyhow!("Upload one MCAP file at a time")));
        }
        let name = field.file_name().unwrap_or("recording.mcap").to_owned();
        let upload_project = project.clone();
        let temporary = blocking(store.clone(), move |store| {
            store.upload_file(&upload_project)
        })
        .await?;
        let file = tokio::fs::File::from_std(
            temporary
                .as_file()
                .try_clone()
                .context("Cannot duplicate the upload file handle")?,
        );
        let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, file);
        let mut digest = Sha256::new();
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|error| ApiError(anyhow::anyhow!("Upload error: {error}")))?
        {
            file.write_all(&chunk)
                .await
                .context("Cannot write the MCAP upload")?;
            digest.update(&chunk);
        }
        file.flush().await.context("Cannot flush the MCAP upload")?;
        drop(file);
        uploaded = Some((name, temporary, format!("{:x}", digest.finalize())));
    }
    let (name, temporary, hash) = uploaded
        .ok_or_else(|| ApiError(anyhow::anyhow!("Upload an MCAP in multipart field file")))?;
    Ok(Json(
        blocking(store, move |store| {
            store.import_uploaded_recording(&project, &name, temporary, hash, options.defer_index)
        })
        .await?,
    ))
}

pub(super) async fn frame(
    State(store): State<Arc<Store>>,
    Path((project, recording, channel, frame)): Path<(String, String, String, usize)>,
) -> ApiResult<Response> {
    let (bytes, mime) = blocking(store, move |store| {
        store.frame(&project, &recording, &channel, frame)
    })
    .await?;
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "private, no-store".to_owned()),
        ],
        Body::from(bytes),
    )
        .into_response())
}

pub(super) async fn record(
    State(store): State<Arc<Store>>,
    Path((project, recording, channel, frame)): Path<(String, String, String, usize)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.record(&project, &recording, &channel, frame)
        })
        .await?,
    ))
}

pub(super) async fn workflow(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.save_workflow(&project, &recording, &request)
        })
        .await?,
    ))
}

pub(super) async fn pipeline(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| store.pipeline(&project, &request)).await?,
    ))
}

pub(super) async fn delete_recording(
    State(store): State<Arc<Store>>,
    Path((project, recording)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            store.delete_recording(&project, &recording)
        })
        .await?,
    ))
}
