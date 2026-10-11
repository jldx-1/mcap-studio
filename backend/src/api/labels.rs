//! Labels HTTP handlers. Domain operations are delegated to Store.
use crate::Store;
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use serde_json::Value;
use std::sync::Arc;

use super::{blocking, ApiResult};

pub(super) async fn update_label(
    State(store): State<Arc<Store>>,
    Path((project, label)): Path<(String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            label_change(store, &project, &actor, |s| {
                s.update_label(&project, &label, &request)
            })
        })
        .await?,
    ))
}

pub(super) async fn label(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            label_change(store, &project, &actor, |s| {
                s.add_named(&project, "labels", &request)
            })
        })
        .await?,
    ))
}

pub(super) async fn label_list(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            label_change(store, &project, &actor, |s| {
                s.add_named(&project, "label_lists", &request)
            })
        })
        .await?,
    ))
}

pub(super) async fn list_label(
    State(store): State<Arc<Store>>,
    Path((project, list)): Path<(String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            label_change(store, &project, &actor, |s| {
                s.add_list_label(&project, &list, &request)
            })
        })
        .await?,
    ))
}

pub(super) async fn update_label_list(
    State(store): State<Arc<Store>>,
    Path((project, list)): Path<(String, String)>,
    Extension(actor): Extension<crate::accounts::Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |store| {
            label_change(store, &project, &actor, |s| {
                s.update_label_list(&project, &list, &request)
            })
        })
        .await?,
    ))
}

fn label_change(
    store: &Store,
    project: &str,
    actor: &crate::accounts::Principal,
    change: impl FnOnce(&Store) -> anyhow::Result<Value>,
) -> anyhow::Result<Value> {
    let before = store.project(project)?;
    let updated = change(store)?;
    if before["labels"] != updated["labels"] || before["label_lists"] != updated["label_lists"] {
        store.invalidate_label_reviews(project, actor)?;
    }
    Ok(updated)
}
