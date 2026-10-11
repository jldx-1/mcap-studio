//! Projects HTTP handlers. Domain operations are delegated to Store.
use crate::Store;
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use serde_json::Value;
use std::sync::Arc;

use super::{blocking, ApiResult};

pub(super) async fn upgrade_project(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| store.upgrade_project(&project)).await?,
    ))
}

pub(super) async fn projects(
    State(store): State<Arc<Store>>,
    Extension(actor): Extension<crate::accounts::Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            let all = s.list_projects()?;
            let mut visible = Vec::new();
            for project in all.as_array().into_iter().flatten() {
                let folder = s.project_folder(project["uuid"].as_str().unwrap_or_default())?;
                if s.accounts
                    .project_permissions(&actor, &folder)?
                    .contains("project.view")
                {
                    visible.push(project.clone());
                }
            }
            Ok(serde_json::json!(visible))
        })
        .await?,
    ))
}

pub(super) async fn create_project(
    State(store): State<Arc<Store>>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| store.create_project(&request)).await?,
    ))
}

pub(super) async fn open_project(
    State(store): State<Arc<Store>>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| store.open_project(&request)).await?,
    ))
}

pub(super) async fn browse_projects(
    State(store): State<Arc<Store>>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| store.browse_projects(&request)).await?,
    ))
}

pub(super) async fn project(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<crate::accounts::Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            let mut value = store.project(&project)?;
            value["permissions"] = serde_json::json!(store
                .accounts
                .project_permissions(&actor, &store.project_folder(&project)?)?);
            Ok(value)
        })
        .await?,
    ))
}
