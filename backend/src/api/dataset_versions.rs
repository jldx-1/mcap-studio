//! Approved data discovery and immutable production dataset versions.
use super::{blocking, ApiResult};
use crate::{accounts::Principal, Store};
use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) async fn approved(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.approved_data(&project)).await?,
    ))
}
pub(super) async fn available(
    State(store): State<Arc<Store>>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            let mut projects = vec![];
            let mut sources = vec![];
            for project in s.list_projects()?.as_array().into_iter().flatten() {
                let id = project["uuid"].as_str().unwrap();
                let folder = s.project_folder(id)?;
                if s.accounts
                    .project_permissions(&actor, &folder)?
                    .contains("project.view")
                {
                    let data = s.approved_data(id)?;
                    projects.push(json!({"uuid":id,"name":project["name"]}));
                    sources.extend(data["sources"].as_array().unwrap().clone());
                }
            }
            Ok(json!({"projects":projects,"sources":sources}))
        })
        .await?,
    ))
}
pub(super) async fn create(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.create_dataset_version(&project, None, &actor, &request)
        })
        .await?,
    ))
}
pub(super) async fn next(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.create_dataset_version(&project, Some(&dataset), &actor, &request)
        })
        .await?,
    ))
}
pub(super) async fn versions(
    State(store): State<Arc<Store>>,
    Path((project, dataset)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.dataset_versions(&project, &dataset)).await?,
    ))
}
pub(super) async fn version(
    State(store): State<Arc<Store>>,
    Path((project, dataset, number)): Path<(String, String, u64)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.dataset_version(&project, &dataset, number)
        })
        .await?,
    ))
}
pub(super) async fn build(
    State(store): State<Arc<Store>>,
    Path((project, dataset, number)): Path<(String, String, u64)>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.build_dataset_version(&project, &dataset, number, &request)
        })
        .await?,
    ))
}
#[derive(serde::Deserialize)]
pub(super) struct PageQuery {
    #[serde(default)]
    offset: usize,
    limit: Option<usize>,
}
pub(super) async fn samples(
    State(store): State<Arc<Store>>,
    Path((project, dataset, number)): Path<(String, String, u64)>,
    Query(query): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.dataset_version_samples(
                &project,
                &dataset,
                number,
                query.offset,
                query.limit.unwrap_or(50),
            )
        })
        .await?,
    ))
}
pub(super) async fn csv(
    State(store): State<Arc<Store>>,
    Path((project, dataset, number)): Path<(String, String, u64)>,
) -> ApiResult<Response> {
    let bytes = blocking(store, move |s| {
        s.dataset_version_csv(&project, &dataset, number)
    })
    .await?;
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
pub(super) async fn input(
    State(store): State<Arc<Store>>,
    Path((project, dataset, number, sample, input)): Path<(String, String, u64, String, String)>,
) -> ApiResult<Response> {
    let (bytes, mime) = blocking(store, move |s| {
        s.dataset_version_input(&project, &dataset, number, &sample, &input)
    })
    .await?;
    Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
}
pub(super) async fn artifacts(
    State(store): State<Arc<Store>>,
    Path((project, dataset, number)): Path<(String, String, u64)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.dataset_version_artifacts(&project, &dataset, number)
        })
        .await?,
    ))
}
