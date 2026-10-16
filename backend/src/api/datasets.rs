//! Datasets HTTP handlers. Domain operations are delegated to Store.
use crate::Store;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::Value;
use std::sync::Arc;

use super::{blocking, ApiError, ApiResult};
use anyhow::Context;
use tokio::io::AsyncWriteExt;

pub(super) async fn import_yolo(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<crate::accounts::Principal>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let staging_project = project.clone();
    let staging = blocking(store.clone(), move |s| {
        s.yolo_upload_directory(&staging_project)
    })
    .await?;
    let (mut name, mut classes, mut files, mut total) =
        (String::new(), Vec::<String>::new(), Vec::new(), 0u64);
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(e.into()))?
    {
        match field.name() {
            Some("name") => {
                name = field.text().await.map_err(|e| ApiError(e.into()))?;
            }
            Some("class_names") => {
                classes =
                    serde_json::from_str(&field.text().await.map_err(|e| ApiError(e.into()))?)
                        .map_err(|e| ApiError(e.into()))?;
            }
            Some("file") => {
                let filename = crate::store::upload_name(
                    field
                        .file_name()
                        .context("Dataset upload needs a filename")?,
                )?;
                if files.len() >= 100_000 {
                    return Err(ApiError(anyhow::anyhow!("Too many dataset files")));
                }
                let path = staging.path().join(format!("file-{}", files.len()));
                let mut output = tokio::fs::File::create(&path)
                    .await
                    .context("Cannot stage dataset file")?;
                let mut size = 0u64;
                while let Some(chunk) = field.chunk().await.map_err(|e| ApiError(e.into()))? {
                    size += chunk.len() as u64;
                    total += chunk.len() as u64;
                    if size > 2 * 1024 * 1024 * 1024 || total > 2 * 1024 * 1024 * 1024 {
                        return Err(ApiError(anyhow::anyhow!("Dataset upload exceeds 2 GiB")));
                    }
                    output
                        .write_all(&chunk)
                        .await
                        .context("Cannot write dataset upload")?;
                }
                output
                    .flush()
                    .await
                    .context("Cannot flush dataset upload")?;
                drop(output);
                files.push((filename, path));
            }
            _ => {}
        }
    }
    Ok(Json(
        blocking(store, move |s| {
            let result = s.import_yolo(&project, &name, &files, &classes, &actor);
            drop(staging);
            result
        })
        .await?,
    ))
}

#[derive(serde::Deserialize)]
pub(super) struct SamplePage {
    #[serde(default)]
    offset: usize,
    #[serde(default = "sample_limit")]
    limit: usize,
}
fn sample_limit() -> usize {
    50
}

pub(super) async fn yolo_samples(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
    Query(page): Query<SamplePage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.yolo_samples(&project, &dataset, page.offset, page.limit)
        })
        .await?,
    ))
}
pub(super) async fn yolo_image(
    State(store): State<Arc<Store>>,
    Path((project, dataset, index)): Path<(String, String, usize)>,
) -> ApiResult<Response> {
    let (bytes, mime) = blocking(store, move |s| s.yolo_image(&project, &dataset, index)).await?;
    Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
}
pub(super) async fn yolo_archive(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
) -> ApiResult<Response> {
    let bytes = blocking(store, move |s| s.yolo_archive(&project, &dataset)).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=yolo-dataset.zip",
            ),
        ],
        bytes,
    )
        .into_response())
}

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
