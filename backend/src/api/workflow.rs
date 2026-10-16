use super::{blocking, ApiResult};
use crate::{accounts::Principal, Store};
use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) async fn list(
    State(store): State<Arc<Store>>,
    Path((project, collection)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.workflow_items(&project, &collection)).await?,
    ))
}
pub(super) async fn save(
    State(store): State<Arc<Store>>,
    Path((project, collection)): Path<(String, String)>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            if collection == "jobs" {
                s.queue_preparation(&project, &request, &actor)
            } else {
                s.save_workflow_item(&project, &collection, &request, &actor)
            }
        })
        .await?,
    ))
}
pub(super) async fn delete(
    State(store): State<Arc<Store>>,
    Path((project, collection, id)): Path<(String, String, String)>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.delete_workflow_item(&project, &collection, &id, &actor)
        })
        .await?,
    ))
}
#[derive(serde::Deserialize)]
pub(super) struct ServerPath {
    #[serde(default)]
    path: String,
}
pub(super) async fn server_files(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<Principal>,
    Query(query): Query<ServerPath>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.server_recordings(&project, &query.path, &actor)
        })
        .await?,
    ))
}
pub(super) async fn server_import(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<ServerPath>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.import_server_recording(&project, &request.path, &actor)
        })
        .await?,
    ))
}
pub(super) async fn library(
    State(store): State<Arc<Store>>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            let mut recordings = vec![];
            for project in s.list_projects()?.as_array().into_iter().flatten() {
                let id = project["uuid"].as_str().unwrap_or_default();
                let permissions = s
                    .accounts
                    .project_permissions(&actor, &s.project_folder(id)?)?;
                if permissions.contains("mcap.view") {
                    let detail = s.project(id)?;
                    for mut recording in detail["mcaps"].as_array().into_iter().flatten().cloned() {
                        recording["project_uuid"] = json!(id);
                        recording["project_name"] = project["name"].clone();
                        recording["can_export"] = json!(permissions.contains("datasets.export"));
                        recordings.push(recording);
                    }
                }
            }
            Ok(json!(recordings))
        })
        .await?,
    ))
}
pub(super) async fn session(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.set_recording_session(&project, &request, &actor)
        })
        .await?,
    ))
}
