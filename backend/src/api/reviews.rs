use super::{blocking, ApiResult};
use crate::{accounts::Principal, Store};
use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use serde_json::Value;
use std::sync::Arc;

#[derive(serde::Deserialize)]
pub(super) struct SampleQuery {
    channel_id: String,
    frame: usize,
    revision: Option<u64>,
}
pub(super) async fn sample(
    State(store): State<Arc<Store>>,
    Path((project, task)): Path<(String, String)>,
    Query(query): Query<SampleQuery>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            if let Some(number) = query.revision {
                s.review_revision_sample(
                    &project,
                    &task,
                    number,
                    &actor,
                    &query.channel_id,
                    query.frame,
                )
            } else {
                s.review_sample(&project, &task, &actor, &query.channel_id, query.frame)
            }
        })
        .await?,
    ))
}

pub(super) async fn revisions(
    State(store): State<Arc<Store>>,
    Path((project, task)): Path<(String, String)>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.review_revisions(&project, &task, &actor)).await?,
    ))
}
pub(super) async fn revision_recording(
    State(store): State<Arc<Store>>,
    Path((project, task, number)): Path<(String, String, u64)>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.review_revision_recording(&project, &task, number, &actor)
        })
        .await?,
    ))
}
#[derive(serde::Deserialize)]
pub(super) struct DiffQuery {
    base: u64,
    #[serde(default)]
    offset: usize,
    limit: Option<usize>,
}
pub(super) async fn revision_diff(
    State(store): State<Arc<Store>>,
    Path((project, task, number)): Path<(String, String, u64)>,
    Query(query): Query<DiffQuery>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.review_revision_diff(
                &project,
                &task,
                number,
                query.base,
                &actor,
                (query.offset, query.limit.unwrap_or(100)),
            )
        })
        .await?,
    ))
}

pub(super) async fn tasks(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.review_tasks(&project, &actor, false)).await?,
    ))
}
pub(super) async fn queue(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.review_tasks(&project, &actor, true)).await?,
    ))
}
pub(super) async fn create_task(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.create_review_task(&project, &actor, &request)
        })
        .await?,
    ))
}
pub(super) async fn action(
    State(store): State<Arc<Store>>,
    Path((project, task)): Path<(String, String)>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.review_task_action(&project, &task, &actor, &request)
        })
        .await?,
    ))
}
