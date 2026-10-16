//! Router composition and HTTP middleware. Resource handlers live in sibling modules.
use crate::Store;
use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue},
    middleware,
    routing::{get, post, put},
    Json, Router,
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

mod accounts;
mod annotations;
mod auth;
mod dataset_versions;
mod datasets;
mod error;
mod exports;
mod labels;
mod projects;
mod recordings;
mod reviews;
mod translations;
mod upload_limit;
mod worker;
mod workflow;

use annotations::{annotations, patch_annotations};
use datasets::{
    build_dataset, create_dataset, dataset, dataset_csv, dataset_entries, dataset_features,
    dataset_pca, delete_dataset, import_yolo, structured_dataset, structured_preview, yolo_archive,
    yolo_image, yolo_samples,
};
pub use error::ApiError;
use error::ApiResult;
use exports::{export, export_download};
use labels::{label, label_list, list_label, update_label, update_label_list};
use projects::{browse_projects, create_project, open_project, project, projects, upgrade_project};
use recordings::{
    delete_recording, frame, import_recording, pipeline, record, recording, workflow,
};
use worker::blocking;

pub fn router(store: Arc<Store>) -> Router {
    let upload_limit =
        upload_limit::UploadLimit::from_env().expect("Invalid MCAP upload limit configuration");
    let frontend = std::env::var_os("MCAP_FRONTEND_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("frontend/dist")
        });
    let origins: Vec<HeaderValue> = std::env::var("MCAP_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:5173,http://127.0.0.1:5173".to_owned())
        .split(',')
        .filter_map(|origin| origin.trim().parse().ok())
        .collect();
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/api/auth/session", get(auth::session))
        .route("/api/auth/setup", post(auth::setup))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/password", put(auth::password))
        .route("/api/auth/profile", put(auth::profile))
        .route(
            "/api/accounts/users",
            get(accounts::users).post(accounts::create_user),
        )
        .route(
            "/api/accounts/users/{user}",
            put(accounts::update_user).delete(accounts::delete_user),
        )
        .route(
            "/api/accounts/groups",
            get(accounts::groups).post(accounts::create_group),
        )
        .route(
            "/api/accounts/users/{user}/projects",
            get(accounts::user_projects).put(accounts::save_user_projects),
        )
        .route(
            "/api/accounts/groups/{group}",
            put(accounts::update_group).delete(accounts::delete_group),
        )
        .route(
            "/api/accounts/settings",
            get(accounts::settings).put(accounts::update_settings),
        )
        .route(
            "/api/projects/{project}/members",
            get(accounts::members).put(accounts::update_members),
        )
        .route(
            "/api/projects/{project}/access-options",
            get(accounts::access_options),
        )
        .route(
            "/api/projects/{project}/directory",
            get(accounts::directory),
        )
        .route(
            "/api/projects/{project}/tasks",
            get(reviews::tasks).post(reviews::create_task),
        )
        .route("/api/projects/{project}/reviews", get(reviews::queue))
        .route(
            "/api/projects/{project}/tasks/{task}/actions",
            post(reviews::action),
        )
        .route(
            "/api/projects/{project}/tasks/{task}/sample",
            get(reviews::sample),
        )
        .route(
            "/api/projects/{project}/tasks/{task}/revisions",
            get(reviews::revisions),
        )
        .route(
            "/api/projects/{project}/tasks/{task}/revisions/{revision}/recording",
            get(reviews::revision_recording),
        )
        .route(
            "/api/projects/{project}/tasks/{task}/revisions/{revision}/diff",
            get(reviews::revision_diff),
        )
        .route("/api/projects", get(projects).post(create_project))
        .route("/api/approved-data", get(dataset_versions::available))
        .route("/api/recording-library", get(workflow::library))
        .route(
            "/api/projects/{project}/workflow/{collection}",
            get(workflow::list).post(workflow::save),
        )
        .route(
            "/api/projects/{project}/workflow/{collection}/{id}",
            axum::routing::delete(workflow::delete),
        )
        .route(
            "/api/projects/{project}/server-recordings",
            get(workflow::server_files).post(workflow::server_import),
        )
        .route(
            "/api/projects/{project}/recordings/session",
            post(workflow::session),
        )
        .route(
            "/api/projects/{project}/approved-data",
            get(dataset_versions::approved),
        )
        .route(
            "/api/projects/{project}/dataset-versions",
            post(dataset_versions::create),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/versions",
            get(dataset_versions::versions).post(dataset_versions::next),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/versions/{version}",
            get(dataset_versions::version),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/versions/{version}/build",
            post(dataset_versions::build),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/versions/{version}/samples",
            get(dataset_versions::samples),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/versions/{version}/csv",
            get(dataset_versions::csv),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/versions/{version}/inputs/{sample}/{input}",
            get(dataset_versions::input),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/versions/{version}/artifacts",
            get(dataset_versions::artifacts),
        )
        .route("/api/projects/open", post(open_project))
        .route("/api/projects/browse", post(browse_projects))
        .route("/api/projects/{project}", get(project))
        .route("/api/projects/{project}/upgrade", post(upgrade_project))
        .route(
            "/api/projects/{project}/upload-limits",
            get(recordings::upload_limits),
        )
        .route(
            "/api/projects/{project}/recordings",
            // Enforce the configured file-byte limit during streaming, rather
            // than counting the multipart envelope against the file size.
            post(import_recording).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}",
            get(recording).delete(delete_recording),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/frames/{channel}/{frame}",
            get(frame),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/records/{channel}/{frame}",
            get(record),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/annotations",
            put(annotations).patch(patch_annotations),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/workflow",
            put(workflow),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/translations",
            get(translations::choices).post(translations::save),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/translations/preview",
            post(translations::preview),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/translations/apply",
            post(translations::apply),
        )
        .route("/api/projects/{project}/labels", post(label))
        .route("/api/projects/{project}/labels/{label}", put(update_label))
        .route("/api/projects/{project}/label-lists", post(label_list))
        .route(
            "/api/projects/{project}/label-lists/{list}",
            put(update_label_list),
        )
        .route("/api/projects/{project}/datasets", post(create_dataset))
        .route(
            "/api/projects/{project}/datasets/import-yolo",
            post(import_yolo),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/samples",
            get(yolo_samples),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/images/{index}",
            get(yolo_image),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/archive",
            get(yolo_archive),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}",
            get(dataset).delete(delete_dataset),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/entries",
            post(dataset_entries),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/build",
            post(build_dataset),
        )
        .route(
            "/api/projects/{project}/label-lists/{list}/labels",
            post(list_label),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/structured/{channel}/preview",
            post(structured_preview),
        )
        .route(
            "/api/projects/{project}/recordings/{recording}/structured/{channel}/datasets",
            post(structured_dataset),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/csv",
            get(dataset_csv),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/features",
            get(dataset_features),
        )
        .route(
            "/api/projects/{project}/datasets/{dataset}/pca",
            post(dataset_pca),
        )
        .route("/api/projects/{project}/pipeline", post(pipeline))
        .route("/api/projects/{project}/exports", post(export))
        .route(
            "/api/projects/{project}/exports/{export}/download",
            get(export_download),
        )
        .fallback_service(
            ServeDir::new(&frontend).not_found_service(ServeFile::new(frontend.join("index.html"))),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
        .layer(axum::Extension(upload_limit))
        .layer(middleware::from_fn_with_state(store.clone(), auth::protect))
        .layer(
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::PATCH,
                    axum::http::Method::DELETE,
                ])
                .allow_credentials(true)
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::HeaderName::from_static("x-requested-with"),
                ]),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(store)
}
