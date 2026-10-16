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
use std::io::{BufWriter, Write};
use std::sync::Arc;

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

pub(super) async fn upload_limits(
    Extension(limit): Extension<super::upload_limit::UploadLimit>,
) -> Json<Value> {
    Json(serde_json::json!({"max_mcap_bytes": limit.max_bytes()}))
}

#[derive(Default, serde::Deserialize)]
pub(super) struct ImportOptions {
    #[serde(default)]
    defer_index: bool,
    #[serde(default = "default_auto_translate")]
    auto_translate: bool,
}
fn default_auto_translate() -> bool {
    true
}
pub(super) async fn import_recording(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Query(options): Query<ImportOptions>,
    Extension(upload_limit): Extension<super::upload_limit::UploadLimit>,
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
        // One blocking worker owns the staged file, buffers disk writes, and
        // hashes the same bytes. The bounded queue applies upload backpressure.
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<axum::body::Bytes>(4);
        let writer = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let mut temporary = temporary;
            let mut digest = Sha256::new();
            {
                let mut file = BufWriter::with_capacity(1024 * 1024, temporary.as_file_mut());
                while let Some(chunk) = receiver.blocking_recv() {
                    file.write_all(&chunk)
                        .context("Cannot write the MCAP upload")?;
                    digest.update(&chunk);
                }
                file.flush().context("Cannot flush the MCAP upload")?;
            }
            Ok((temporary, format!("{:x}", digest.finalize())))
        });
        let transfer = async {
            let mut received = 0;
            while let Some(chunk) = field.chunk().await.context("Cannot read the MCAP upload")? {
                received = upload_limit.add(received, chunk.len())?;
                // Even a large upstream body chunk cannot grow the queue without bound.
                for offset in (0..chunk.len()).step_by(1024 * 1024) {
                    sender
                        .send(chunk.slice(offset..(offset + 1024 * 1024).min(chunk.len())))
                        .await
                        .context("MCAP upload writer stopped")?;
                }
            }
            anyhow::Ok(())
        }
        .await;
        drop(sender);
        let written = writer.await.context("MCAP upload writer failed")?;
        // Prefer a concrete disk error over the resulting closed-channel error.
        let (temporary, hash) = written?;
        transfer?;
        uploaded = Some((name, temporary, hash));
    }
    let (name, temporary, hash) = uploaded
        .ok_or_else(|| ApiError(anyhow::anyhow!("Upload an MCAP in multipart field file")))?;
    Ok(Json(
        blocking(store, move |store| {
            let imported = store.import_uploaded_recording(
                &project,
                &name,
                temporary,
                hash,
                options.defer_index,
            )?;
            Ok(if options.auto_translate {
                store.auto_translate_import(&project, imported)
            } else {
                imported
            })
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
