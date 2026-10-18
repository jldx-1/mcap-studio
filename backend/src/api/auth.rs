//! Authentication, CSRF checks and route-level permissions. Mutation services also
//! check ownership and transitions while holding the project lock.
use super::{blocking, ApiError, ApiResult};
use crate::{accounts::Principal, Store};
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, Method},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| part.trim().strip_prefix("mcap_session=").map(str::to_owned))
}
fn cookie(value: &str, clear: bool) -> String {
    let secure = if std::env::var("MCAP_SECURE_COOKIES").as_deref() == Ok("1") {
        "; Secure"
    } else {
        ""
    };
    format!(
        "mcap_session={value}; HttpOnly; SameSite=Strict; Path=/{secure}{}",
        if clear { "; Max-Age=0" } else { "" }
    )
}
pub(super) async fn session(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let token = token(&headers);
    Ok(Json(blocking(store, move |s| {
        let user = token.as_deref().and_then(|t| s.accounts.authenticate(t).ok());
        Ok(json!({"setup_required":s.accounts.setup_required()?,"user":user,"permission_catalog":crate::accounts::PERMISSIONS,"workflow":s.accounts.settings()?}))
    }).await?))
}
pub(super) async fn setup(
    State(store): State<Arc<Store>>,
    Json(request): Json<Value>,
) -> ApiResult<Response> {
    let (user, token) = blocking(store, move |s| s.accounts.setup(&request)).await?;
    Ok(([(header::SET_COOKIE, cookie(&token, false))], Json(user)).into_response())
}
pub(super) async fn profile(
    State(store): State<Arc<Store>>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Json<Principal>> {
    Ok(Json(
        blocking(store, move |s| s.accounts.update_profile(&actor, &request)).await?,
    ))
}
pub(super) async fn login(
    State(store): State<Arc<Store>>,
    Json(request): Json<Value>,
) -> ApiResult<Response> {
    let (user, token) = blocking(store, move |s| s.accounts.login(&request)).await?;
    Ok(([(header::SET_COOKIE, cookie(&token, false))], Json(user)).into_response())
}
pub(super) async fn logout(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if let Some(token) =
        token(&headers).filter(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        blocking(store, move |s| s.accounts.logout(&token)).await?;
    }
    Ok((
        [(header::SET_COOKIE, cookie("", true))],
        Json(json!({"ok":true})),
    )
        .into_response())
}
pub(super) async fn password(
    State(store): State<Arc<Store>>,
    Extension(actor): Extension<Principal>,
    Json(request): Json<Value>,
) -> ApiResult<Response> {
    blocking(store, move |s| s.accounts.change_password(&actor, &request)).await?;
    Ok((
        [(header::SET_COOKIE, cookie("", true))],
        Json(json!({"ok":true})),
    )
        .into_response())
}

pub(super) async fn protect(
    State(store): State<Arc<Store>>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    if !path.starts_with("/api/") || request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        if request
            .headers()
            .get("x-requested-with")
            .and_then(|h| h.to_str().ok())
            != Some("mcap-workbench")
        {
            return ApiError(anyhow::anyhow!(
                "Forbidden: X-Requested-With header required"
            ))
            .into_response();
        }
        if let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok())
        {
            let configured = std::env::var("MCAP_ALLOWED_ORIGINS")
                .unwrap_or_else(|_| "http://localhost:5173,http://127.0.0.1:5173".into());
            let host = request
                .headers()
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("");
            if origin != format!("http://{host}")
                && origin != format!("https://{host}")
                && !configured.split(',').any(|o| o.trim() == origin)
            {
                return ApiError(anyhow::anyhow!("Forbidden: request origin is not allowed"))
                    .into_response();
            }
        }
    }
    if matches!(
        path.as_str(),
        "/api/health"
            | "/api/auth/session"
            | "/api/auth/setup"
            | "/api/auth/login"
            | "/api/auth/logout"
    ) {
        return private(next.run(request).await);
    }
    let Some(token) = token(request.headers()) else {
        return ApiError(anyhow::anyhow!("Unauthenticated: sign in to continue")).into_response();
    };
    let method = request.method().clone();
    let result = blocking(store, move |s| {
        let actor = s.accounts.authenticate(&token)?;
        authorize_route(s, &actor, &path, &method)?;
        Ok(actor)
    })
    .await;
    match result {
        Ok(actor) => {
            request.extensions_mut().insert(actor);
            private(next.run(request).await)
        }
        Err(error) => error.into_response(),
    }
}

fn private(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    response
}

fn authorize_route(
    store: &Store,
    actor: &Principal,
    path: &str,
    method: &Method,
) -> anyhow::Result<()> {
    let parts: Vec<_> = path.trim_matches('/').split('/').collect();
    if parts.get(1) == Some(&"auth") {
        return Ok(());
    }
    if matches!(parts.get(1), Some(&"approved-data" | &"recording-library")) {
        return Ok(());
    }
    if parts.get(1) == Some(&"accounts") {
        let permission = match parts.get(2).copied() {
            Some("users") => "users.manage",
            Some("groups") => {
                if *method == Method::GET && actor.can("users.manage") {
                    "users.manage"
                } else {
                    "groups.manage"
                }
            }
            Some("settings") => "settings.manage",
            _ => {
                anyhow::bail!("Forbidden: unknown account resource");
            }
        };
        anyhow::ensure!(actor.can(permission), "Forbidden: {permission} required");
        return Ok(());
    }
    anyhow::ensure!(
        parts.get(1) == Some(&"projects"),
        "Forbidden: unknown API resource"
    );
    if parts.len() == 2 && *method == Method::GET {
        return Ok(());
    }
    let project = parts.get(2).copied().unwrap_or("");
    if project.is_empty() || matches!(project, "open" | "browse") {
        anyhow::ensure!(
            actor.can("projects.admin"),
            "Forbidden: projects.admin required"
        );
        return Ok(());
    }
    let folder = store.project_folder(project)?;
    let read = matches!(*method, Method::GET | Method::HEAD);
    let permission = match parts.get(3).copied() {
        None => "project.view",
        Some("members" | "access-options" | "translation-definitions") => "project.manage",
        Some("directory") => "project.view",
        Some("tasks") => {
            if read
                && store
                    .accounts
                    .project_permissions(actor, &folder)?
                    .contains("review.view")
            {
                "review.view"
            } else if read {
                "annotation.view"
            } else {
                "annotation.submit"
            }
        } // actions checked individually below
        Some("reviews") => "review.view",
        Some("upgrade" | "pipeline" | "upload-limits") => "project.manage",
        Some("labels" | "label-lists") => "labels.manage",
        Some("exports") => "datasets.export",
        Some("approved-data") => "project.view",
        Some("server-recordings") => "project.manage",
        Some("workflow") => {
            if read && parts.get(4) == Some(&"drafts") {
                "project.view"
            } else if read {
                "mcap.view"
            } else if parts.get(4) == Some(&"drafts") {
                "datasets.generate"
            } else {
                "project.manage"
            }
        }
        Some("dataset-versions") => "datasets.generate",
        Some("datasets") => {
            if read {
                if matches!(parts.last(), Some(&"csv" | &"archive")) {
                    "datasets.export"
                } else {
                    "project.view"
                }
            } else {
                "datasets.generate"
            }
        }
        Some("recordings") => {
            if read {
                "mcap.view"
            } else {
                match parts.get(5).copied() {
                    Some("annotations") => "annotation.view",
                    Some("structured") => {
                        if parts.last() == Some(&"datasets") {
                            "datasets.generate"
                        } else {
                            "annotation.view"
                        }
                    }
                    _ => "project.manage",
                }
            }
        }
        _ => {
            anyhow::bail!("Forbidden: unknown project resource");
        }
    };
    // Reviewers can inspect task details and act without annotation.submit.
    if parts.get(3) == Some(&"tasks") && parts.len() > 4 {
        let permissions = store.accounts.project_permissions(actor, &folder)?;
        anyhow::ensure!(
            permissions.contains("project.view")
                && (permissions.contains("review.view") || permissions.contains("annotation.view")),
            "Forbidden: task access required"
        );
        return Ok(());
    }
    store.accounts.require_project(actor, &folder, permission)
}
