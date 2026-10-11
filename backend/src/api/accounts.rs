use super::{blocking, ApiResult};
use crate::{accounts::Principal, Store};
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) async fn users(State(store): State<Arc<Store>>) -> ApiResult<Json<Value>> {
    Ok(Json(blocking(store, |s| s.accounts.users()).await?))
}
pub(super) async fn delete_user(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    blocking(store, move |s| s.accounts.delete_user(&actor, &id)).await?;
    Ok(Json(json!({"ok":true})))
}
pub(super) async fn user_projects(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.user_project_assignments(&actor, &id)).await?,
    ))
}
pub(super) async fn save_user_projects(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.save_user_projects(&actor, &id, &request)).await?,
    ))
}
pub(super) async fn groups(State(store): State<Arc<Store>>) -> ApiResult<Json<Value>> {
    Ok(Json(blocking(store, |s| s.accounts.groups()).await?))
}
pub(super) async fn settings(State(store): State<Arc<Store>>) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, |s| Ok(serde_json::to_value(s.accounts.settings()?)?)).await?,
    ))
}
pub(super) async fn create_user(
    State(store): State<Arc<Store>>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.accounts.save_user(&actor, None, &request)).await?,
    ))
}
pub(super) async fn update_user(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.accounts.save_user(&actor, Some(&id), &request)
        })
        .await?,
    ))
}
pub(super) async fn create_group(
    State(store): State<Arc<Store>>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.accounts.save_group(&actor, None, &request)
        })
        .await?,
    ))
}
pub(super) async fn update_group(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.accounts.save_group(&actor, Some(&id), &request)
        })
        .await?,
    ))
}
pub(super) async fn delete_group(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    blocking(store, move |s| s.accounts.delete_group(&actor, &id)).await?;
    Ok(Json(json!({"ok":true})))
}
pub(super) async fn update_settings(
    State(store): State<Arc<Store>>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.accounts.save_settings(&actor, &request)).await?,
    ))
}
pub(super) async fn members(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| s.accounts.members(&s.project_folder(&id)?)).await?,
    ))
}
pub(super) async fn update_members(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.accounts
                .save_members(&actor, &s.project_folder(&id)?, &request)
        })
        .await?,
    ))
}
pub(super) async fn directory(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, move |s| {
            s.accounts.project_directory(&s.project_folder(&id)?)
        })
        .await?,
    ))
}
pub(super) async fn access_options(
    State(store): State<Arc<Store>>,
    Path(_id): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        blocking(store, |s| {
            let mut users = s.accounts.users()?;
            for user in users.as_array_mut().unwrap() {
                user.as_object_mut().unwrap().remove("permissions");
                user.as_object_mut().unwrap().remove("groups");
            }
            Ok(json!({"users":users,"groups":s.accounts.groups()?}))
        })
        .await?,
    ))
}
