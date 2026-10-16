//! Browser requests through a same-origin web proxy must use server storage,
//! while uploads carry client file bytes without requiring a client-local path.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use mcap_workbench::{router, Store};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Cursor, sync::Arc};
use tower::ServiceExt;

fn request(
    method: &str,
    path: &str,
    body: Body,
    content_type: &str,
    cookie: &str,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "studio.example.test")
        .header("origin", "https://studio.example.test")
        .header("x-requested-with", "mcap-workbench")
        .header("cookie", cookie)
        .header("content-type", content_type)
        .body(body)
        .unwrap()
}
async fn json_response(app: &Router, request: Request<Body>) -> Value {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn remote_browser_can_locate_server_projects_upload_client_bytes_and_view_frames() {
    let dir = tempfile::tempdir().unwrap();
    let server = dir.path().join("backend-storage");
    let store = Arc::new(Store::new(&server).unwrap());
    let (_, token) = store
        .accounts
        .setup(&json!({"username":"admin","name":"Admin","password":"remote-web-test-password"}))
        .unwrap();
    let cookie = format!("mcap_session={token}");
    let project = store
        .create_project(&json!({"name":"Server project"}))
        .unwrap();
    let id = project["uuid"].as_str().unwrap();
    let app = router(store);
    let listing = json_response(
        &app,
        request(
            "POST",
            "/api/projects/browse",
            Body::from(json!({"path":server.join("data/projects")}).to_string()),
            "application/json",
            &cookie,
        ),
    )
    .await;
    assert!(listing["folders"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["path"] == project["path"] && item["is_project"] == true));
    let opened = json_response(
        &app,
        request(
            "POST",
            "/api/projects/open",
            Body::from(json!({"path":project["path"]}).to_string()),
            "application/json",
            &cookie,
        ),
    )
    .await;
    assert_eq!(opened["uuid"], project["uuid"]);
    let limits = json_response(
        &app,
        request(
            "GET",
            &format!("/api/projects/{id}/upload-limits"),
            Body::empty(),
            "application/json",
            &cookie,
        ),
    )
    .await;
    assert!(limits.as_object().unwrap().contains_key("max_mcap_bytes"));

    let mut writer = mcap::Writer::new(Cursor::new(Vec::new())).unwrap();
    let channel = writer
        .add_channel(0, "/camera", "json", &BTreeMap::new())
        .unwrap();
    writer
        .write_to_known_channel(
            &mcap::records::MessageHeader {
                channel_id: channel,
                sequence: 0,
                log_time: 1,
                publish_time: 1,
            },
            &serde_json::to_vec(&json!({"width":1,"height":1,"encoding":"rgb8","data":[255,0,0]}))
                .unwrap(),
        )
        .unwrap();
    writer.finish().unwrap();
    let source = writer.into_inner().into_inner();
    let mut form = b"--remote\r\nContent-Disposition: form-data; name=\"file\"; filename=\"laptop-recording.mcap\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    form.extend(source);
    form.extend(b"\r\n--remote--\r\n");
    let imported = json_response(
        &app,
        request(
            "POST",
            &format!("/api/projects/{id}/recordings?defer_index=true&auto_translate=false"),
            Body::from(form),
            "multipart/form-data; boundary=remote",
            &cookie,
        ),
    )
    .await;
    assert_eq!(imported["source_file"], "laptop-recording.mcap");
    let recording = imported["uuid"].as_str().unwrap();
    let indexed = json_response(
        &app,
        request(
            "GET",
            &format!("/api/projects/{id}/recordings/{recording}"),
            Body::empty(),
            "application/json",
            &cookie,
        ),
    )
    .await;
    assert_eq!(indexed["topics"][0]["frame_count"], 1);
    let channel = indexed["topics"][0]["channel_id"].as_str().unwrap();
    let response = app
        .oneshot(request(
            "GET",
            &format!("/api/projects/{id}/recordings/{recording}/frames/{channel}/0"),
            Body::empty(),
            "application/json",
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.starts_with(b"\x89PNG"));
}
