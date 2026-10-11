use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use mcap_workbench::{
    router as protected_router,
    store::{contained_path, file_hash, validate_annotations, write_json},
    Store,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::Path,
    sync::Arc,
};
use tower::ServiceExt;

// Existing endpoint regression tests use a real administrator session. Dedicated
// account tests exercise the protected router without this request helper.
fn router(store: Arc<Store>) -> axum::Router {
    let first = store.accounts.setup_required().unwrap();
    let request =
        json!({"username":"test-admin","name":"Test administrator","password":"test-password-123"});
    let (actor, token) = if first {
        store.accounts.setup(&request).unwrap()
    } else {
        store.accounts.login(&request).unwrap()
    };
    if first {
        store.accounts.save_settings(&actor,&json!({"independent_reviewer":false,"require_approval_for_datasets":false,"session_hours":12})).unwrap();
    }
    protected_router(store).layer(axum::middleware::from_fn(
        move |mut request: axum::extract::Request, next: axum::middleware::Next| {
            request
                .headers_mut()
                .insert("cookie", format!("mcap_session={token}").parse().unwrap());
            request
                .headers_mut()
                .insert("x-requested-with", "mcap-workbench".parse().unwrap());
            async move { next.run(request).await }
        },
    ))
}

fn fixture(path: &Path) {
    fixture_with_count(path, 3);
}

fn fixture_with_count(path: &Path, count: u32) {
    let mut writer = mcap::Writer::new(File::create(path).unwrap()).unwrap();
    let image = writer
        .add_channel(0, "/camera", "json", &BTreeMap::new())
        .unwrap();
    let record = writer
        .add_channel(0, "/motor", "json", &BTreeMap::new())
        .unwrap();
    for number in 0..count {
        let time = 1_791_057_600_000_000_001 + number as u64 * 1_000;
        for (channel, payload) in [
            (
                image,
                json!({"width":2,"height":2,"step":6,"encoding":"rgb8","data":[255,0,0,255,0,0,255,0,0,255,0,0]}),
            ),
            (record, json!({"position":number,"sample_ns":time})),
        ] {
            writer
                .write_to_known_channel(
                    &mcap::records::MessageHeader {
                        channel_id: channel,
                        sequence: number,
                        log_time: time,
                        publish_time: time,
                    },
                    &serde_json::to_vec(&payload).unwrap(),
                )
                .unwrap();
        }
    }
    writer.finish().unwrap();
}

fn imported(store: &Store, workspace: &Path) -> (String, Value) {
    // These tests also exercise externally edited legacy manifests and annotations.
    let project = store
        .create_project(&json!({"name":"Robot lab", "format_version":2}))
        .unwrap();
    let id = project["uuid"].as_str().unwrap().to_owned();
    fixture(&workspace.join("sample.mcap"));
    let recording = store
        .import_recording(&id, "sample.mcap", &workspace.join("sample.mcap"))
        .unwrap();
    (id, recording)
}

fn imported_v3(store: &Store, workspace: &Path, count: u32) -> (String, Value) {
    let project = store
        .create_project(&json!({"name":"Incremental robot"}))
        .unwrap();
    assert_eq!(project["format_version"], 3);
    let id = project["uuid"].as_str().unwrap().to_owned();
    fixture_with_count(&workspace.join("sample.mcap"), count);
    let recording = store
        .import_recording(&id, "sample.mcap", &workspace.join("sample.mcap"))
        .unwrap();
    (id, recording)
}

#[test]
fn compact_annotation_saves_preserve_other_pages_without_loading_them() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported_v3(&store, workspace.path(), 257);
    let recording_id = recording["uuid"].as_str().unwrap();
    let folder = store.project(&id).unwrap()["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let folder = Path::new(&folder);
    let mut state = recording["annotations"].clone();
    let row = json!({"reviewed":false,"boxes":[],"labels":[]});
    state["techniques"]["1"] = json!("yolo");
    state["annotations"] = json!({"1":{"0":row,"256":row}});
    store.save_annotations(&id, recording_id, &state).unwrap();
    let metadata_path = folder.join(format!("annotations/{recording_id}/metadata.json"));
    let before = disk_json(&metadata_path);
    let other = folder.join(before["pages"]["1/000000000001"]["path"].as_str().unwrap());
    let retired = other.with_extension("offline");
    fs::rename(&other, &retired).unwrap();
    let changed = json!({"reviewed":true,"boxes":[],"labels":[]});
    let patch = json!({"sha256":recording["sha256"],"return_delta":true,"changes":[{"channel_id":"1","frame":0,"annotation":changed,"expected":row}]});
    let response = store.patch_annotations(&id, recording_id, &patch).unwrap();
    assert!(response.get("annotations").is_none());
    assert_eq!(response["changes"][0]["annotation"], changed);
    assert_eq!(
        disk_json(&metadata_path)["pages"]["1/000000000001"],
        before["pages"]["1/000000000001"]
    );
    assert!(store.patch_annotations(&id, recording_id, &patch).is_err());
    // Previewing a video frame also has no dependency on annotation pages.
    assert!(store.frame(&id, recording_id, "1", 256).is_ok());
    fs::rename(&retired, &other).unwrap();
    let saved = store.recording(&id, recording_id).unwrap();
    assert_eq!(saved["annotations"]["annotations"]["1"]["0"], changed);
    assert_eq!(saved["annotations"]["annotations"]["1"]["256"], row);
    let stamp = mcap_workbench::storage::fingerprint(&metadata_path).unwrap();
    store
        .patch_annotations(
            &id,
            recording_id,
            &json!({"sha256":recording["sha256"],"return_delta":true,"changes":[]}),
        )
        .unwrap();
    assert_eq!(
        stamp,
        mcap_workbench::storage::fingerprint(&metadata_path).unwrap()
    );
}

#[test]
fn annotation_conflicts_compare_browser_numbers_without_rounding_timestamps() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    store
        .add_named(&id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let mut state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    state["techniques"]["1"] = json!("yolo");
    state["settings"]["step"] = json!(1.0);
    let row = json!({"reviewed":false,"boxes":[{"x":0.0,"y":0.0,"width":1.0,"height":1.0,"class_id":0,"track_id":1}],"labels":[],"fields":{"sample_ns":1791057600000000001u64}});
    state["annotations"] = json!({"1":{"0":row}});
    store.save_annotations(&id, recording_id, &state).unwrap();
    let browser_row = json!({"reviewed":false,"boxes":[{"x":0,"y":0,"width":1,"height":1,"class_id":0,"track_id":1}],"labels":[],"fields":{"sample_ns":1791057600000000001u64}});
    let mut changed = browser_row.clone();
    changed["reviewed"] = json!(true);
    store.patch_annotations(&id,recording_id,&json!({"sha256":recording["sha256"],"return_delta":true,"changes":[{"channel_id":"1","frame":0,"expected":browser_row,"annotation":changed}],"metadata":{"settings":{"step":1,"positions":{"1":0}}},"expected_metadata":{"settings":{"step":1,"positions":{}}}})).unwrap();
    let mut wrong = changed.clone();
    wrong["fields"]["sample_ns"] = json!(1791057600000000002u64);
    assert!(store.patch_annotations(&id,recording_id,&json!({"sha256":recording["sha256"],"return_delta":true,"changes":[{"channel_id":"1","frame":0,"expected":wrong,"annotation":changed}]})).is_err());
}

#[tokio::test]
async fn deferred_upload_is_saved_before_extraction_and_opens_lazily() {
    let workspace = tempfile::tempdir().unwrap();
    check_deferred_upload(workspace.path()).await;
}

#[tokio::test]
async fn deferred_upload_publishes_on_the_project_filesystem() {
    // Exercise the actual project filesystem too: Windows/DrvFS rejects moving
    // a recording directory while its validation file handle is still open.
    let workspace = tempfile::Builder::new()
        .prefix(".storage-check-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    check_deferred_upload(workspace.path()).await;
}

async fn check_deferred_upload(workspace: &Path) {
    let store = Arc::new(Store::new(workspace).unwrap());
    let project = store
        .create_project(&json!({"name":"Deferred upload"}))
        .unwrap();
    let id = project["uuid"].as_str().unwrap();
    let source = workspace.join("source.mcap");
    fixture(&source);
    let bytes = fs::read(&source).unwrap();
    let mut body=b"--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"source.mcap\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    body.extend_from_slice(&bytes);
    body.extend_from_slice(b"\r\n--boundary--\r\n");
    let response = router(store.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/projects/{id}/recordings?defer_index=true"))
                .header("content-type", "multipart/form-data; boundary=boundary")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(saved["indexing"], "pending");
    let recording_id = saved["uuid"].as_str().unwrap();
    let folder = Path::new(project["path"].as_str().unwrap());
    assert_eq!(
        fs::read(folder.join(saved["path"].as_str().unwrap())).unwrap(),
        bytes
    );
    assert!(!folder.join(format!("mcap/{recording_id}/frames")).exists());
    assert_eq!(
        store.project(id).unwrap()["mcaps"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let opened = store.recording(id, recording_id).unwrap();
    assert_eq!(opened["topics"].as_array().unwrap().len(), 2);
    assert!(folder
        .join(format!("mcap/{recording_id}/frames/index.json"))
        .is_file());
    assert_eq!(saved["sha256"], file_hash(&source).unwrap());
}

fn disk_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn v3_annotations_only_publish_changed_pages_and_reject_conflicting_patches() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported_v3(&store, workspace.path(), 257);
    let recording_id = recording["uuid"].as_str().unwrap();
    let project = store
        .add_named(&id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let folder = Path::new(project["path"].as_str().unwrap());
    let root = mcap_workbench::storage::fingerprint(&folder.join("project.json")).unwrap();
    let source = folder.join(recording["path"].as_str().unwrap());
    let source_stamp = mcap_workbench::storage::fingerprint(&source).unwrap();
    let mut state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    state["techniques"]["1"] = json!("yolo");
    let row = json!({"reviewed":true,"boxes":[{"x":0,"y":0,"width":1,"height":1,"class_id":0,"track_id":1}],"labels":[]});
    state["annotations"] = json!({"1":{"0":row,"256":row}});
    store.save_annotations(&id, recording_id, &state).unwrap();
    let path = folder.join(format!("annotations/{recording_id}/metadata.json"));
    let before = disk_json(&path);
    let second_page = folder.join(before["pages"]["1/000000000001"]["path"].as_str().unwrap());
    let second_stamp = mcap_workbench::storage::fingerprint(&second_page).unwrap();
    let mut changed = row.clone();
    changed["boxes"][0]["x"] = json!(0.5);
    let patch = json!({"sha256":recording["sha256"],"changes":[{"channel_id":"1","frame":0,"expected":row,"annotation":changed}]});
    store.patch_annotations(&id, recording_id, &patch).unwrap();
    assert!(store.patch_annotations(&id, recording_id, &patch).is_err());
    let after = disk_json(&path);
    assert_ne!(
        before["pages"]["1/000000000000"],
        after["pages"]["1/000000000000"]
    );
    assert_eq!(
        before["pages"]["1/000000000001"],
        after["pages"]["1/000000000001"]
    );
    assert_eq!(
        second_stamp,
        mcap_workbench::storage::fingerprint(&second_page).unwrap()
    );
    let state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    let annotation_stamp = mcap_workbench::storage::fingerprint(&path).unwrap();
    store.save_annotations(&id, recording_id, &state).unwrap();
    assert_eq!(
        annotation_stamp,
        mcap_workbench::storage::fingerprint(&path).unwrap()
    );
    assert_eq!(
        root,
        mcap_workbench::storage::fingerprint(&folder.join("project.json")).unwrap()
    );
    assert_eq!(
        source_stamp,
        mcap_workbench::storage::fingerprint(&source).unwrap()
    );
    let reloaded = Store::new(workspace.path()).unwrap();
    assert_eq!(
        reloaded.recording(&id, recording_id).unwrap()["annotations"]["annotations"]["1"]["0"],
        changed
    );
}

#[test]
fn v3_dataset_builds_reuse_images_and_track_only_relevant_changes() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported_v3(&store, workspace.path(), 3);
    let recording_id = recording["uuid"].as_str().unwrap();
    let project = store
        .add_named(&id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let label = project["labels"][0]["uuid"].as_str().unwrap();
    let mut state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    state["techniques"]["1"] = json!("yolo");
    state["annotations"] = json!({"1":{"0":{"reviewed":true,"boxes":[{"x":0,"y":0,"width":1,"height":1,"class_id":0,"track_id":1}],"labels":[]},"1":{"reviewed":true,"boxes":[],"labels":[]}}});
    store.save_annotations(&id, recording_id, &state).unwrap();
    let project = store
        .add_named(&id, "datasets", &json!({"name":"Detection"}))
        .unwrap();
    let dataset = project["datasets"][0]["uuid"].as_str().unwrap();
    let project = store
        .add_dataset_entries(
            &id,
            dataset,
            &json!({"recording_uuid":recording_id,"channel_id":"1"}),
        )
        .unwrap();
    let folder = Path::new(project["path"].as_str().unwrap());
    let root = disk_json(&folder.join("project.json"));
    assert!(root["datasets"][dataset].get("entries").is_none());
    assert_eq!(
        store.dataset(&id, dataset).unwrap()["entries"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert!(store.delete_recording(&id, recording_id).is_err());
    let first = store.build_dataset(&id, dataset, &json!({})).unwrap();
    assert_eq!(first["copied_images"], 2);
    assert_eq!(first["updated_samples"], 2);
    let second = store.build_dataset(&id, dataset, &json!({})).unwrap();
    assert_eq!(second["copied_images"], 0);
    assert_eq!(second["updated_samples"], 0);
    let sample_image = folder.join(format!(
        "datasets/{dataset}/generated/yolo/images/{recording_id}_1_000000000000.png"
    ));
    let image_stamp = mcap_workbench::storage::fingerprint(&sample_image).unwrap();
    let membership_path = folder.join(format!("datasets/{dataset}/dataset.json"));
    let membership_stamp = mcap_workbench::storage::fingerprint(&membership_path).unwrap();
    store
        .update_label(&id, label, &json!({"name":"arm"}))
        .unwrap();
    assert_eq!(
        membership_stamp,
        mcap_workbench::storage::fingerprint(&membership_path).unwrap()
    );
    assert_eq!(
        store.build_dataset(&id, dataset, &json!({})).unwrap()["updated_samples"],
        0
    );
    assert!(fs::read_to_string(
        folder.join(format!("datasets/{dataset}/generated/yolo/data.yaml"))
    )
    .unwrap()
    .contains("arm"));
    let mut state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    state["annotations"]["1"]["0"]["boxes"][0]["x"] = json!(0.5);
    store.save_annotations(&id, recording_id, &state).unwrap();
    let changed = store.build_dataset(&id, dataset, &json!({})).unwrap();
    assert_eq!(changed["updated_samples"], 1);
    assert_eq!(changed["copied_images"], 0);
    assert_eq!(
        image_stamp,
        mcap_workbench::storage::fingerprint(&sample_image).unwrap()
    );
    assert_eq!(disk_json(&folder.join("project.json")), root);
    let mut workflow = store.recording(&id, recording_id).unwrap()["workflow"].clone();
    workflow["translations"]["2"] = json!("position -> motor");
    store.save_workflow(&id, recording_id, &workflow).unwrap();
    assert_eq!(
        store.build_dataset(&id, dataset, &json!({})).unwrap()["updated_samples"],
        0
    );
    workflow["translations"]["1"] = json!("* -> *");
    store.save_workflow(&id, recording_id, &workflow).unwrap();
    assert_eq!(
        store.build_dataset(&id, dataset, &json!({})).unwrap()["updated_samples"],
        2
    );
    state["annotations"]["1"]["0"]["reviewed"] = json!(false);
    store.save_annotations(&id, recording_id, &state).unwrap();
    let removed = store.build_dataset(&id, dataset, &json!({})).unwrap();
    assert_eq!(removed["removed_samples"], 1);
    assert!(!sample_image.exists());
    let reloaded = Store::new(workspace.path()).unwrap();
    assert_eq!(
        reloaded.project(&id).unwrap()["datasets"][0]["entry_count"],
        2
    );
    assert_eq!(
        reloaded
            .build_dataset(&id, dataset, &json!({"format":"csv"}))
            .unwrap()["sample_count"],
        1
    );
}

#[test]
fn v3_csv_pages_survive_unrelated_edits_and_can_be_rebuilt() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported_v3(&store, workspace.path(), 3);
    let recording_id = recording["uuid"].as_str().unwrap();
    let project = store.save_structured_dataset(&id, recording_id, "2", &json!({"name":"Motor","columns":[{"id":"position","name":"position","source":"/position","transform":"number"}]})).unwrap();
    let dataset = project["datasets"][0]["uuid"].as_str().unwrap();
    assert_eq!(
        store.build_dataset(&id, dataset, &json!({})).unwrap()["updated_samples"],
        3
    );
    store
        .add_named(&id, "labels", &json!({"name":"unused"}))
        .unwrap();
    let reloaded = Store::new(workspace.path()).unwrap();
    assert_eq!(
        reloaded.dataset(&id, dataset).unwrap()["entries"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        reloaded.build_dataset(&id, dataset, &json!({})).unwrap()["updated_samples"],
        0
    );
    let folder = Path::new(project["path"].as_str().unwrap());
    fs::remove_dir_all(folder.join(format!("datasets/{dataset}/generated"))).unwrap();
    assert_eq!(
        reloaded.build_dataset(&id, dataset, &json!({})).unwrap()["sample_count"],
        3
    );
    let csv = String::from_utf8(reloaded.dataset_csv(&id, dataset).unwrap()).unwrap();
    let mut lines = csv.lines();
    assert_eq!(lines.next(), Some("\"position\""));
    assert_eq!(
        lines
            .map(|line| line.trim_matches('"').parse::<f64>().unwrap())
            .collect::<Vec<_>>(),
        vec![0.0, 1.0, 2.0]
    );
}

#[test]
fn v2_upgrade_preserves_recordings_and_dataset_provenance() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    let mut state = recording["annotations"].clone();
    state["techniques"]["1"] = json!("yolo");
    state["annotations"] = json!({"1":{"0":{"reviewed":true,"boxes":[],"labels":[]}}});
    store.save_annotations(&id, recording_id, &state).unwrap();
    let project = store
        .add_named(&id, "datasets", &json!({"name":"Legacy"}))
        .unwrap();
    let dataset = project["datasets"][0]["uuid"].as_str().unwrap();
    store
        .add_dataset_entries(
            &id,
            dataset,
            &json!({"recording_uuid":recording_id,"channel_id":"1"}),
        )
        .unwrap();
    let folder = Path::new(project["path"].as_str().unwrap());
    let source = folder.join(recording["path"].as_str().unwrap());
    let source_stamp = mcap_workbench::storage::fingerprint(&source).unwrap();
    assert_eq!(store.upgrade_project(&id).unwrap()["format_version"], 3);
    assert!(folder.join("project.v2.backup.json").is_file());
    assert_eq!(
        source_stamp,
        mcap_workbench::storage::fingerprint(&source).unwrap()
    );
    let reloaded = Store::new(workspace.path()).unwrap();
    let entry = reloaded.dataset(&id, dataset).unwrap()["entries"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert_eq!(
        entry["labeled_data"]["path"],
        format!("annotations/{recording_id}/metadata.json")
    );
    assert_eq!(
        reloaded.build_dataset(&id, dataset, &json!({})).unwrap()["sample_count"],
        1
    );
    let project = mcap_workbench::Project::open(folder).unwrap();
    assert_eq!(project.dataset("Legacy").unwrap()["uuid"], dataset);
}

#[test]
fn v3_cache_invalidation_observes_external_metadata_and_recording_changes() {
    use std::io::{Seek, SeekFrom, Write};
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported_v3(&store, workspace.path(), 3);
    let project = store
        .add_named(&id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let folder = Path::new(project["path"].as_str().unwrap());
    let label = &project["labels"][0];
    let path = folder.join(label["path"].as_str().unwrap());
    let mut edited = disk_json(&path);
    edited["name"] = json!("Updated on disk");
    write_json(&path, &edited).unwrap();
    assert_eq!(
        store.project(&id).unwrap()["labels"][0]["name"],
        "Updated on disk"
    );
    let source = folder.join(recording["path"].as_str().unwrap());
    let mut file = fs::OpenOptions::new().write(true).open(&source).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"BADMAGIC").unwrap();
    file.sync_all().unwrap();
    assert!(store
        .save_annotations(
            &id,
            recording["uuid"].as_str().unwrap(),
            &recording["annotations"]
        )
        .is_err());
}

#[tokio::test]
async fn v3_http_patch_conflicts_and_dataset_builds_are_available() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(workspace.path()).unwrap());
    let (id, recording) = imported_v3(&store, workspace.path(), 3);
    let recording_id = recording["uuid"].as_str().unwrap();
    let app = router(store.clone());
    let patch = json!({"sha256":recording["sha256"], "changes":[{"channel_id":"1","frame":0,"annotation":{"reviewed":true,"boxes":[],"labels":[]},"expected":null}], "metadata":{"techniques":{"1":"yolo","2":"undefined"}}});
    for expected in [StatusCode::OK, StatusCode::CONFLICT] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!(
                        "/api/projects/{id}/recordings/{recording_id}/annotations"
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(patch.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let project = store
        .add_named(&id, "datasets", &json!({"name":"HTTP training"}))
        .unwrap();
    let dataset = project["datasets"][0]["uuid"].as_str().unwrap();
    store
        .add_dataset_entries(
            &id,
            dataset,
            &json!({"recording_uuid":recording_id,"channel_id":"1"}),
        )
        .unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/projects/{id}/datasets/{dataset}/build"))
                .header("content-type", "application/json")
                .body(Body::from("{\"format\":\"yolo\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["sample_count"], 1);
}

#[test]
fn portable_registries_survive_reload_and_reject_duplicate_membership() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let project = store
        .create_project(&json!({"name":"Shared project"}))
        .unwrap();
    let id = project["uuid"].as_str().unwrap();
    let project = store
        .add_named(id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let label = &project["labels"][0]["uuid"];
    assert!(store
        .add_named(id, "labels", &json!({"name":"robot"}))
        .is_err());
    assert!(store
        .add_named(
            id,
            "label_lists",
            &json!({"name":"Invalid","label_ids":[label,label]})
        )
        .is_err());
    store
        .add_named(
            id,
            "label_lists",
            &json!({"name":"Objects","label_ids":[label]}),
        )
        .unwrap();
    store
        .add_named(id, "datasets", &json!({"name":"Training"}))
        .unwrap();
    let reloaded = Store::new(workspace.path()).unwrap();
    let projects = reloaded.list_projects().unwrap();
    assert_eq!(projects[0]["label_count"], 1);
    assert_eq!(projects[0]["dataset_count"], 1);
    assert_eq!(
        reloaded.project(id).unwrap()["label_lists"][0]["label_ids"][0],
        *label
    );
    assert!(contained_path(workspace.path(), "../../outside").is_err());
    assert!(contained_path(workspace.path(), "/tmp/outside").is_err());
}

#[cfg(unix)]
#[test]
fn path_containment_checks_symlinks_before_creating_files() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), workspace.path().join("data")).unwrap();
    assert!(contained_path(workspace.path(), "data/projects/new").is_err());
    let store = Store::new(workspace.path()).unwrap();
    assert!(store
        .create_project(&json!({"name":"Cannot escape"}))
        .is_err());
    assert!(!outside.path().join("projects").exists());
}

#[test]
fn reviewed_frames_include_negatives_with_idempotent_provenance_and_real_exports() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    store
        .add_named(&id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let mut state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    state["techniques"]["1"] = json!("yolo");
    let bbox =
        json!({"x":0,"y":0,"width":1,"height":1,"class_id":0,"track_id":1,"origin":"manual"});
    state["annotations"]["1"] = json!({"0":{"reviewed":true,"boxes":[bbox],"labels":[]},"1":{"reviewed":true,"boxes":[],"labels":[]},"2":{"reviewed":false,"boxes":[],"labels":[]}});
    state["tracks"] = json!({"1":{"1":{"start":0,"end":null,"keyframes":{"0":bbox}}}});
    store.save_annotations(&id, recording_id, &state).unwrap();
    let tracks = state.as_object_mut().unwrap().remove("tracks").unwrap();
    assert_eq!(
        store.save_annotations(&id, recording_id, &state).unwrap()["tracks"],
        tracks
    );
    let mut invalid = state.clone();
    invalid["annotations"]["1"]["0"]["boxes"][0]["width"] = json!(3);
    assert!(store.save_annotations(&id, recording_id, &invalid).is_err());
    invalid = state.clone();
    invalid["classes"][0] = json!("renamed");
    assert!(store.save_annotations(&id, recording_id, &invalid).is_err());
    invalid = state.clone();
    invalid["sha256"] = json!("0".repeat(64));
    assert!(store.save_annotations(&id, recording_id, &invalid).is_err());
    invalid = state.clone();
    invalid["techniques"]["1"] = json!("classification");
    assert!(store.save_annotations(&id, recording_id, &invalid).is_err());
    invalid = state.clone();
    invalid["tracks"] = tracks.clone();
    invalid["tracks"]["1"]["1"]["end"] = json!(100);
    assert!(store.save_annotations(&id, recording_id, &invalid).is_err());
    let empty_list = store
        .add_named(
            &id,
            "label_lists",
            &json!({"name":"No classes","label_ids":[]}),
        )
        .unwrap();
    invalid = state.clone();
    invalid["topic_label_lists"] = json!({"1":empty_list["label_lists"][0]["uuid"]});
    assert!(store.save_annotations(&id, recording_id, &invalid).is_err());
    store
        .add_named(&id, "labels", &json!({"name":"second"}))
        .unwrap();
    let mut swapped = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    swapped["class_uuids"].as_array_mut().unwrap().swap(0, 1);
    assert!(store.save_annotations(&id, recording_id, &swapped).is_err());
    let project = store
        .add_named(&id, "datasets", &json!({"name":"Training"}))
        .unwrap();
    let dataset_id = project["datasets"][0]["uuid"].as_str().unwrap();
    let request = json!({"recording_uuid":recording_id,"channel_id":"1"});
    for _ in 0..2 {
        let project = store
            .add_dataset_entries(&id, dataset_id, &request)
            .unwrap();
        assert_eq!(project["datasets"][0]["entry_count"], 2);
    }
    let dataset = store.dataset(&id, dataset_id).unwrap();
    let entries = dataset["entries"].as_object().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.values().next().unwrap()["raw_data"]["sha256"],
        recording["sha256"]
    );
    for format in ["coco", "yolo", "jsonl"] {
        let result = store
            .export(
                &id,
                &json!({"recording_uuid":recording_id,"channel_id":"1","format":format}),
            )
            .unwrap();
        assert_eq!(result["reviewed_count"], 2);
        let bytes = store
            .export_zip(&id, result["uuid"].as_str().unwrap())
            .unwrap();
        let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<_> = archive.file_names().collect();
        assert!(names.contains(&"channel_1/images/00000000.png"));
        assert!(names.contains(&"channel_1/images/00000001.png"));
        assert!(!names.contains(&"channel_1/images/00000002.png"));
    }
    let reloaded = Store::new(workspace.path()).unwrap();
    assert_eq!(
        reloaded.dataset(&id, dataset_id).unwrap()["entries"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn recording_hash_and_cache_identity_are_checked_before_labels_are_served() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let rid = recording["uuid"].as_str().unwrap();
    let project = store.project(&id).unwrap();
    let folder = Path::new(project["path"].as_str().unwrap());
    let path = contained_path(folder, recording["path"].as_str().unwrap()).unwrap();
    let hash = file_hash(&path).unwrap();
    assert_eq!(hash, recording["sha256"].as_str().unwrap());
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(1);
    fs::write(&path, bytes).unwrap();
    assert!(store
        .recording(&id, rid)
        .unwrap_err()
        .to_string()
        .contains("SHA-256"));
}

#[test]
fn discovery_opens_existing_projects_without_rewriting_reference_files() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let project = store.project(&id).unwrap();
    let folder = Path::new(project["path"].as_str().unwrap());
    let original_manifest = fs::read(folder.join("project.json")).unwrap();
    let original_recording =
        fs::read(contained_path(folder, recording["path"].as_str().unwrap()).unwrap()).unwrap();
    let selected = workspace.path().join("External project");
    fs::rename(folder, &selected).unwrap();
    let reloaded = Store::new(workspace.path()).unwrap();
    assert_eq!(
        reloaded.list_projects().unwrap().as_array().unwrap().len(),
        1
    );
    reloaded.open_project(&json!({"path":selected})).unwrap();
    reloaded
        .recording(&id, recording["uuid"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        fs::read(selected.join("project.json")).unwrap(),
        original_manifest
    );
    assert_eq!(
        fs::read(contained_path(&selected, recording["path"].as_str().unwrap()).unwrap()).unwrap(),
        original_recording
    );
    let invalid = workspace.path().join("Invalid project");
    fs::create_dir(&invalid).unwrap();
    write_json(
        &invalid.join("project.json"),
        &json!({"kind":"mcap-labeling-project","format_version":2,"uuid":"not-a-uuid"}),
    )
    .unwrap();
    assert_eq!(
        Store::new(workspace.path())
            .unwrap()
            .list_projects()
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn http_errors_are_actionable_and_timestamps_keep_full_precision() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(workspace.path()).unwrap());
    let (id, recording) = imported(&store, workspace.path());
    assert_eq!(
        recording["index"]["topics"]["1"]["frames"][0]["time_ns"],
        "1791057600000000001"
    );
    let app = router(store);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/projects/{id}/recordings/{}/frames/1/999",
                    recording["uuid"].as_str().unwrap()
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(serde_json::from_slice::<Value>(&body).unwrap()["error"]
        .as_str()
        .unwrap()
        .contains("outside"));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/projects/00000000-0000-4000-8000-000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn annotation_validation_rejects_unknown_topics_noninteger_ids_and_bad_frames() {
    let index = json!({"sha256":"hash","topics":{"1":{"kind":"image","errors":[],"frames":[{"width":2,"height":2}]}}});
    let project = json!({"labels":{},"label_lists":{}});
    let state = json!({"format_version":1,"sha256":"hash","classes":["robot"],"techniques":{"1":"coco"},"annotations":{"1":{"0":{"reviewed":true,"boxes":[{"x":0,"y":0,"width":1,"height":1,"class_id":0,"track_id":1}],"labels":[]}}}});
    validate_annotations(&index, &project, &state, None).unwrap();
    let mut invalid = state.clone();
    invalid["annotations"]["1"]["0"]["boxes"][0]["class_id"] = json!(0.1);
    assert!(validate_annotations(&index, &project, &invalid, None).is_err());
    invalid = state.clone();
    invalid["techniques"]["2"] = json!("coco");
    assert!(validate_annotations(&index, &project, &invalid, None).is_err());
    invalid = state.clone();
    invalid["annotations"]["1"]["5"] = invalid["annotations"]["1"]["0"].clone();
    assert!(validate_annotations(&index, &project, &invalid, None).is_err());
}

#[test]
fn processed_recordings_have_a_fresh_workflow_for_renumbered_channels() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let request = json!({"recording_uuid":recording["uuid"],"selected_channels":["2"],"translations":{"1":"deselected mapping remains saved","2":"* -> *\nvalue <- position * 2"},"sync":{"enabled":false}});
    let processed = store.pipeline(&id, &request).unwrap();
    assert_eq!(processed["workflow"]["selected_channels"], json!(["1"]));
    assert_eq!(processed["workflow"]["translations"], json!({}));
    assert_eq!(processed["workflow"]["sync"]["enabled"], false);
    assert_eq!(processed["source_recording_uuid"], recording["uuid"]);
    let second=store.pipeline(&id,&json!({"recording_uuid":processed["uuid"],"selected_channels":["1"],"translations":{},"sync":{"enabled":false}})).unwrap();
    assert_eq!(
        store
            .record(&id, second["uuid"].as_str().unwrap(), "1", 1)
            .unwrap()["value"],
        2.0
    );
    assert_eq!(
        store.project(&id).unwrap()["mcaps"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn cached_project_reads_observe_external_manifest_updates_and_revalidate_them() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let project = store.project(&id).unwrap();
    let path = Path::new(project["path"].as_str().unwrap()).join("project.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["name"] = json!("Changed by the Python reference app");
    write_json(&path, &manifest).unwrap();
    assert_eq!(store.project(&id).unwrap()["name"], manifest["name"]);
    manifest["mcaps"][recording["uuid"].as_str().unwrap()]["path"] = json!("../../outside.mcap");
    write_json(&path, &manifest).unwrap();
    assert!(store.project(&id).is_err());
}

#[cfg(unix)]
#[test]
fn cached_project_validation_never_allows_a_recording_symlink_to_escape() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    store.frame(&id, recording_id, "1", 0).unwrap();
    let project = store.project(&id).unwrap();
    let path =
        Path::new(project["path"].as_str().unwrap()).join(recording["path"].as_str().unwrap());
    fs::copy(&path, outside.path().join("source.mcap")).unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(outside.path().join("source.mcap"), &path).unwrap();
    assert!(store.frame(&id, recording_id, "1", 0).is_err());
    assert!(store
        .save_annotations(&id, recording_id, &recording["annotations"])
        .is_err());
}

#[tokio::test]
async fn folder_browser_finds_projects_without_loading_or_registering_them() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let external_store = Store::new(outside.path()).unwrap();
    let external = external_store
        .create_project(&json!({"name":"Desktop project"}))
        .unwrap();
    let external_path = external["path"].as_str().unwrap();
    let parent = Path::new(external_path).parent().unwrap();
    fs::create_dir(parent.join("ordinary-folder")).unwrap();
    fs::create_dir(parent.join(".hidden-folder")).unwrap();
    fs::write(parent.join("not-a-folder.txt"), "ignored").unwrap();
    let store = Arc::new(Store::new(workspace.path()).unwrap());
    let app = router(store.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/projects/browse")
                .header("content-type", "application/json")
                .body(Body::from(json!({"path":parent}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["path"], json!(parent.canonicalize().unwrap()));
    assert_eq!(body["folders"].as_array().unwrap().len(), 2);
    assert_eq!(body["folders"][0]["is_project"], true);
    assert_eq!(body["folders"][0]["path"], external_path);
    assert_eq!(body["shortcuts"][0]["name"], "Workspace");
    assert!(store
        .list_projects()
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    let project_listing = store
        .browse_projects(&json!({"path":external_path}))
        .unwrap();
    assert_eq!(project_listing["is_project"], true);
    assert!(store
        .browse_projects(&json!({"path":parent.join("not-a-folder.txt")}))
        .is_err());
    assert!(store
        .browse_projects(&json!({"path":parent.join("missing")}))
        .is_err());
    store.open_project(&json!({"path":external_path})).unwrap();
    assert_eq!(store.list_projects().unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn folder_browser_offers_windows_desktops_for_a_wsl_workspace() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("Users/desktop-user");
    let desktop = home.join("Desktop");
    let cloud_desktop = home.join("OneDrive/Desktop");
    let workspace = home.join("Downloads/workspace");
    for folder in [&desktop, &cloud_desktop, &workspace] {
        fs::create_dir_all(folder).unwrap();
    }
    let store = Store::new(&workspace).unwrap();
    let listing = store.browse_projects(&json!({})).unwrap();
    let shortcuts = listing["shortcuts"].as_array().unwrap();
    assert!(shortcuts
        .iter()
        .any(|item| item["name"] == "Home" && item["path"] == json!(home)));
    for path in [desktop, cloud_desktop] {
        assert!(shortcuts
            .iter()
            .any(|item| item["name"] == "Desktop" && item["path"] == json!(path)));
    }
}

#[test]
fn project_summary_reports_duration_modalities_and_saved_review_progress() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    let project = store.project(&id).unwrap();
    let summary = &project["mcaps"][0]["summary"];
    assert_eq!(summary["topic_count"], 2);
    assert_eq!(summary["frame_count"], 6);
    assert_eq!(summary["topic_names"], json!(["/camera", "/motor"]));
    assert!(summary["modalities"]
        .as_array()
        .unwrap()
        .contains(&json!("image")));
    assert!((summary["duration_seconds"].as_f64().unwrap() - 0.000002).abs() < 1e-12);
    assert_eq!(summary["reviewed_count"], 0);
    assert!(project["mcaps"][0]["created_at"].is_string());

    let mut state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    state["techniques"]["2"] = json!("csv");
    state["annotations"]["2"] = json!({
        "0": {"reviewed": true, "boxes": [], "labels": [], "fields": {"status":"ok"}},
        "1": {"reviewed": true, "boxes": [], "labels": []}
    });
    state["settings"]["step"] = json!(30);
    store.save_annotations(&id, recording_id, &state).unwrap();
    let refreshed = store.project(&id).unwrap();
    assert_eq!(refreshed["mcaps"][0]["summary"]["annotated_count"], 1);
    assert_eq!(refreshed["mcaps"][0]["summary"]["reviewed_count"], 2);
    assert_eq!(
        store.recording(&id, recording_id).unwrap()["annotations"]["settings"]["step"],
        30
    );
}

#[test]
fn label_lists_own_labels_and_reuse_existing_vocabulary() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let project = store.create_project(&json!({"name":"Lists"})).unwrap();
    let id = project["uuid"].as_str().unwrap();
    let project = store
        .add_named(
            id,
            "label_lists",
            &json!({"name":"Vehicle Types","label_ids":[]}),
        )
        .unwrap();
    let list = project["label_lists"][0]["uuid"].as_str().unwrap();
    let project = store
        .add_list_label(id, list, &json!({"name":"Car"}))
        .unwrap();
    assert_eq!(project["labels"].as_array().unwrap().len(), 1);
    assert_eq!(
        project["label_lists"][0]["label_ids"][0],
        project["labels"][0]["uuid"]
    );
    assert!(store
        .add_list_label(id, list, &json!({"name":"Car"}))
        .is_err());
    let project = store
        .add_named(
            id,
            "label_lists",
            &json!({"name":"Object Detection","label_ids":[]}),
        )
        .unwrap();
    let other = project["label_lists"]
        .as_array()
        .unwrap()
        .iter()
        .find(|list| list["name"] == "Object Detection")
        .unwrap()["uuid"]
        .as_str()
        .unwrap();
    let project = store
        .add_list_label(id, other, &json!({"name":"Car"}))
        .unwrap();
    assert_eq!(project["labels"].as_array().unwrap().len(), 1);
    assert_eq!(
        project["label_lists"][0]["label_ids"],
        project["label_lists"][1]["label_ids"]
    );
    assert_eq!(
        Store::new(workspace.path()).unwrap().project(id).unwrap()["label_lists"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn structured_datasets_transform_rows_preserve_precision_and_quote_csv() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    let preview = store
        .structured_preview(&id, recording_id, "2", &json!({"offset":1,"limit":1}))
        .unwrap();
    assert_eq!(preview["total"], 3);
    assert_eq!(preview["rows"].as_array().unwrap().len(), 1);
    assert_eq!(preview["rows"][0]["index"], 1);
    assert_eq!(preview["rows"][0]["values"]["/position"], 1);
    assert_eq!(
        preview["rows"][0]["values"]["/sample_ns"],
        "1791057600000001001"
    );
    assert!(store
        .structured_preview(&id, recording_id, "1", &json!({}))
        .is_err());
    let request = json!({"name":"Motor CSV", "columns":[
        {"id":"position","name":"scaled_position","source":"/position","transform":"scale","scale":2,"offset":10},
        {"id":"stamp","name":"timestamp","source":"/sample_ns","transform":"identity"},
        {"id":"truth","name":"truth","source":null,"transform":"boolean","default":"false"},
        {"id":"note","name":"note","source":null,"default":""}
    ], "overrides":{"1":{"truth":"yes","note":"good, \"clean\"\nnext line"}}});
    let project = store
        .save_structured_dataset(&id, recording_id, "2", &request)
        .unwrap();
    let dataset_id = project["datasets"][0]["uuid"].as_str().unwrap();
    assert_eq!(project["datasets"][0]["entry_count"], 3);
    let reloaded = Store::new(workspace.path()).unwrap();
    let dataset = reloaded.dataset(&id, dataset_id).unwrap();
    let row = &dataset["entries"][format!("{recording_id}:2:1")];
    assert_eq!(row["values"]["scaled_position"], 12.0);
    assert_eq!(row["values"]["truth"], true);
    assert_eq!(row["values"]["timestamp"], "1791057600000001001");
    assert_eq!(row["raw_data"]["sha256"], recording["sha256"]);
    let csv = String::from_utf8(reloaded.dataset_csv(&id, dataset_id).unwrap()).unwrap();
    assert!(csv.starts_with("\"scaled_position\",\"timestamp\",\"truth\",\"note\"\r\n"));
    assert!(csv.contains("\"good, \"\"clean\"\"\nnext line\""));
    assert_eq!(csv.matches("1791057600000001001").count(), 1);
    let mut invalid = request.clone();
    invalid["name"] = json!("Bad columns");
    invalid["columns"][1]["name"] = json!("scaled_position");
    assert!(store
        .save_structured_dataset(&id, recording_id, "2", &invalid)
        .is_err());
    invalid = request.clone();
    invalid["name"] = json!("Bad truth");
    invalid["overrides"]["1"]["truth"] = json!("maybe");
    assert!(store
        .save_structured_dataset(&id, recording_id, "2", &invalid)
        .is_err());
    invalid = request.clone();
    invalid["name"] = json!("Bad field");
    invalid["columns"][0]["source"] = json!("/missing");
    assert!(store
        .save_structured_dataset(&id, recording_id, "2", &invalid)
        .is_err());
    assert_eq!(
        store.project(&id).unwrap()["datasets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut edited = request.clone();
    edited["dataset_uuid"] = json!(dataset_id);
    edited["overrides"]["1"]["note"] = json!("updated truth note");
    store
        .save_structured_dataset(&id, recording_id, "2", &edited)
        .unwrap();
    let dataset = Store::new(workspace.path())
        .unwrap()
        .dataset(&id, dataset_id)
        .unwrap();
    assert_eq!(dataset["overrides"]["1"]["note"], "updated truth note");
    assert_eq!(
        dataset["entries"][format!("{recording_id}:2:1")]["values"]["note"],
        "updated truth note"
    );
    assert_eq!(
        store.project(&id).unwrap()["datasets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn protobuf_topics_flatten_nested_fields_into_csv_rows() {
    let workspace = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../utils/synthetic_robot_arm.mcap");
    let bytes = fs::read(source).unwrap();
    let messages = mcap::MessageStream::new(&bytes)
        .unwrap()
        .map(Result::unwrap)
        .filter(|message| message.channel.id == 8)
        .take(3)
        .collect::<Vec<_>>();
    let path = workspace.path().join("proto.mcap");
    let mut writer = mcap::Writer::new(File::create(&path).unwrap()).unwrap();
    let schema = messages[0].channel.schema.as_ref().unwrap();
    let schema_id = writer
        .add_schema(&schema.name, &schema.encoding, &schema.data)
        .unwrap();
    let channel_id = writer
        .add_channel(
            schema_id,
            &messages[0].channel.topic,
            "protobuf",
            &BTreeMap::new(),
        )
        .unwrap();
    for message in &messages {
        writer
            .write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id,
                    sequence: message.sequence,
                    log_time: message.log_time,
                    publish_time: message.publish_time,
                },
                &message.data,
            )
            .unwrap();
    }
    writer.finish().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let project = store.create_project(&json!({"name":"Proto CSV"})).unwrap();
    let id = project["uuid"].as_str().unwrap();
    let recording = store.import_recording(id, "proto.mcap", &path).unwrap();
    let recording_id = recording["uuid"].as_str().unwrap();
    let channel = channel_id.to_string();
    let preview = store
        .structured_preview(id, recording_id, &channel, &json!({}))
        .unwrap();
    assert_eq!(preview["total"], 3);
    assert!(
        preview["fields"]
            .as_array()
            .unwrap()
            .contains(&json!("/timestamp/nanos")),
        "{}",
        preview
    );
    let project = store.save_structured_dataset(id, recording_id, &channel, &json!({"name":"Positions", "columns":[{"id":"x","name":"x_mm","source":"/x_m","transform":"scale","scale":1000,"offset":0}],"overrides":{}})).unwrap();
    let dataset_id = project["datasets"][0]["uuid"].as_str().unwrap();
    let dataset = store.dataset(id, dataset_id).unwrap();
    assert_eq!(dataset["entries"].as_object().unwrap().len(), 3);
    assert!(
        dataset["entries"][format!("{recording_id}:{channel}:0")]["values"]["x_mm"].is_number()
    );
}

#[tokio::test]
async fn structured_http_preview_and_csv_download_are_available() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(workspace.path()).unwrap());
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    let app = router(store.clone());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/projects/{id}/recordings/{recording_id}/structured/2/preview"
                ))
                .header("content-type", "application/json")
                .body(Body::from("{\"limit\":1}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["rows"].as_array().unwrap().len(), 1);
    let project = store.save_structured_dataset(&id, recording_id, "2", &json!({"name":"HTTP CSV", "columns":[{"id":"p","name":"position","source":"/position"}],"overrides":{}})).unwrap();
    let dataset_id = project["datasets"][0]["uuid"].as_str().unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/projects/{id}/datasets/{dataset_id}/csv"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/csv; charset=utf-8"
    );
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=dataset.csv"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8(body.to_vec())
        .unwrap()
        .starts_with("\"position\"\r\n"));
    let mut state = recording["annotations"].clone();
    state["techniques"]["2"] = json!("classification");
    assert!(store.save_annotations(&id, recording_id, &state).is_err());
    state = recording["annotations"].clone();
    state["techniques"]["1"] = json!("coco");
    assert!(store.save_annotations(&id, recording_id, &state).is_err());
}

#[test]
fn label_list_order_and_removal_preserve_existing_annotations() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    let project = store
        .add_named(
            &id,
            "label_lists",
            &json!({"name":"Vehicles","label_ids":[]}),
        )
        .unwrap();
    let list_id = project["label_lists"][0]["uuid"].as_str().unwrap();
    store
        .add_list_label(&id, list_id, &json!({"name":"Car"}))
        .unwrap();
    let project = store
        .add_list_label(&id, list_id, &json!({"name":"Truck"}))
        .unwrap();
    let ids = project["label_lists"][0]["label_ids"].as_array().unwrap();
    let car = ids[0].clone();
    let truck = ids[1].clone();
    let mut state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    let class_id = state["class_uuids"]
        .as_array()
        .unwrap()
        .iter()
        .position(|id| id == &car)
        .unwrap();
    state["techniques"]["1"] = json!("classification");
    state["topic_label_lists"] = json!({"1":list_id});
    state["topic_labels"] = json!({"1":ids});
    state["annotations"]["1"] = json!({"0":{"reviewed":true,"boxes":[],"labels":[class_id]}});
    store.save_annotations(&id, recording_id, &state).unwrap();
    let project = store
        .update_label_list(
            &id,
            list_id,
            &json!({"label_ids":[truck,car],"expected_label_ids":ids}),
        )
        .unwrap();
    assert_eq!(project["label_lists"][0]["label_ids"], json!([truck, car]));
    assert!(store
        .update_label_list(
            &id,
            list_id,
            &json!({"label_ids":[],"expected_label_ids":ids})
        )
        .is_err());
    assert!(store
        .update_label_list(&id, list_id, &json!({"label_ids":[car,car]}))
        .is_err());
    let project = store
        .update_label_list(&id, list_id, &json!({"label_ids":[truck]}))
        .unwrap();
    assert_eq!(project["label_lists"][0]["label_ids"], json!([truck]));
    assert_eq!(project["labels"].as_array().unwrap().len(), 2);
    // A previously labeled frame remains valid after its label is removed from the list.
    store.save_annotations(&id, recording_id, &state).unwrap();
    let reloaded = Store::new(workspace.path()).unwrap();
    let saved = reloaded.recording(&id, recording_id).unwrap();
    assert_eq!(
        saved["annotations"]["annotations"]["1"]["0"]["labels"],
        json!([class_id])
    );
    let project = reloaded
        .add_list_label(&id, list_id, &json!({"name":"Car"}))
        .unwrap();
    assert_eq!(project["label_lists"][0]["label_ids"], json!([truck, car]));
    assert_eq!(project["labels"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn datasets_report_sources_project_numeric_distribution_and_delete_only_dataset_files() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(workspace.path()).unwrap());
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    let annotations = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    let project = store
        .save_structured_dataset(
            &id,
            recording_id,
            "2",
            &json!({"name":"Distribution", "columns":[
        {"id":"p","name":"position","source":"/position"},
        {"id":"q","name":"quality","source":null,"transform":"boolean","default":"false"}
    ],"overrides":{"1":{"q":"true"}}}),
        )
        .unwrap();
    let dataset = &project["datasets"][0];
    let dataset_id = dataset["uuid"].as_str().unwrap();
    assert_eq!(dataset["modalities"], json!(["structured"]));
    assert_eq!(dataset["multimodal"], false);
    assert_eq!(dataset["sources"][0]["mcap_uuid"], recording_id);
    assert_eq!(dataset["sources"][0]["topic"], "/motor");
    assert_eq!(dataset["sources"][0]["entry_count"], 3);
    let fields = store.dataset_features(&id, dataset_id).unwrap();
    assert_eq!(fields["fields"], json!(["/position", "/quality"]));
    let pca = store
        .dataset_pca(
            &id,
            dataset_id,
            &json!({"features":["/position","/quality"],"standardize":true}),
        )
        .unwrap();
    assert_eq!(pca["points"].as_array().unwrap().len(), 3);
    assert!((pca["explained_variance_ratio"][0].as_f64().unwrap() - 0.5).abs() < 1e-10);
    assert_eq!(pca["points"][0]["mcap_uuid"], recording_id);
    assert!(store
        .dataset_pca(
            &id,
            dataset_id,
            &json!({"features":["/position","/missing"]})
        )
        .is_err());
    let app = router(store.clone());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/projects/{id}/datasets/{dataset_id}/pca"))
                .header("content-type", "application/json")
                .body(Body::from("{\"features\":[\"/position\",\"/quality\"]}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let folder = std::path::PathBuf::from(project["path"].as_str().unwrap());
    let directory = folder.join(format!("datasets/{dataset_id}"));
    assert!(directory.join("data.csv").is_file());
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/projects/{id}/datasets/{dataset_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!directory.exists());
    assert!(store.dataset(&id, dataset_id).is_err());
    let reloaded = Store::new(workspace.path()).unwrap();
    assert_eq!(reloaded.project(&id).unwrap()["datasets"], json!([]));
    assert_eq!(
        reloaded.recording(&id, recording_id).unwrap()["annotations"],
        annotations
    );
    assert!(folder
        .join(format!("mcap/{recording_id}/recording.mcap"))
        .is_file());
}

#[test]
fn dataset_modalities_follow_included_topics_and_visual_pca_uses_image_labels() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    store
        .add_named(&id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let mut state = store.recording(&id, recording_id).unwrap()["annotations"].clone();
    state["techniques"]["1"] = json!("yolo");
    state["annotations"]["1"] = json!({
        "0":{"reviewed":true,"boxes":[{"x":0,"y":0,"width":1,"height":1,"class_id":0,"track_id":1}],"labels":[]},
        "1":{"reviewed":true,"boxes":[],"labels":[]},
        "2":{"reviewed":true,"boxes":[],"labels":[]}
    });
    store.save_annotations(&id, recording_id, &state).unwrap();
    let project = store
        .add_named(&id, "datasets", &json!({"name":"Visual"}))
        .unwrap();
    let dataset_id = project["datasets"][0]["uuid"].as_str().unwrap();
    let project = store
        .add_dataset_entries(
            &id,
            dataset_id,
            &json!({"recording_uuid":recording_id,"channel_id":"1"}),
        )
        .unwrap();
    assert_eq!(project["datasets"][0]["modalities"], json!(["image"]));
    let pca = store
        .dataset_pca(
            &id,
            dataset_id,
            &json!({"features":["/box_count","/mean_box_area"]}),
        )
        .unwrap();
    assert_eq!(pca["points"].as_array().unwrap().len(), 3);
    assert!((pca["explained_variance_ratio"][0].as_f64().unwrap() - 1.0).abs() < 1e-10);
    // Existing mixed datasets include both image and structured topic membership.
    let folder = std::path::PathBuf::from(project["path"].as_str().unwrap());
    let path = folder.join("project.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut entry =
        manifest["datasets"][dataset_id]["entries"][format!("{recording_id}:1:0")].clone();
    entry["channel_id"] = json!("2");
    entry["topic"] = json!("/motor");
    entry["raw_data"].as_object_mut().unwrap().remove("image");
    entry["raw_data"]["records"] = json!(format!(
        "mcap/{recording_id}/frames/channel_2/records.jsonl"
    ));
    entry["labeled_data"]["channel_id"] = json!("2");
    manifest["datasets"][dataset_id]["entries"][format!("{recording_id}:2:0")] = entry;
    write_json(&path, &manifest).unwrap();
    let dataset = store.dataset(&id, dataset_id).unwrap();
    assert_eq!(dataset["multimodal"], true);
    assert_eq!(dataset["modalities"], json!(["image", "structured"]));
    assert_eq!(dataset["sources"].as_array().unwrap().len(), 2);
    let scoped = store.dataset_pca(&id, dataset_id, &json!({"features":["/box_count","/mean_box_area"],"source":format!("{recording_id}:1")})).unwrap();
    assert_eq!(scoped["points"].as_array().unwrap().len(), 3);
    assert_eq!(scoped["candidate_entries"], 3);
}

#[cfg(unix)]
#[test]
fn dataset_deletion_rejects_storage_symlinks_to_recording_data() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    let project = store
        .add_named(&id, "datasets", &json!({"name":"Symlink test"}))
        .unwrap();
    let dataset_id = project["datasets"][0]["uuid"].as_str().unwrap();
    let folder = std::path::PathBuf::from(project["path"].as_str().unwrap());
    fs::create_dir_all(folder.join("datasets")).unwrap();
    let recording_dir = folder.join(format!("mcap/{recording_id}"));
    std::os::unix::fs::symlink(
        &recording_dir,
        folder.join(format!("datasets/{dataset_id}")),
    )
    .unwrap();
    assert!(store.delete_dataset(&id, dataset_id).is_err());
    assert!(recording_dir.join("recording.mcap").is_file());
    assert!(store.dataset(&id, dataset_id).is_ok());
}

#[test]
fn unused_legacy_message_classification_does_not_block_image_datasets_or_exports() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Store::new(workspace.path()).unwrap();
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    let mut annotations = recording["annotations"].clone();
    annotations["techniques"]["1"] = json!("yolo");
    annotations["techniques"]["2"] = json!("classification");
    annotations["annotations"]["1"] = json!({"0":{"reviewed":true,"boxes":[],"labels":[]}});
    let project = store.project(&id).unwrap();
    let path = std::path::PathBuf::from(project["path"].as_str().unwrap())
        .join(format!("mcap/{recording_id}/annotations.json"));
    write_json(&path, &annotations).unwrap();
    let before = fs::read(&path).unwrap();
    let loaded = store.recording(&id, recording_id).unwrap();
    assert_eq!(loaded["annotations"]["techniques"]["2"], "csv");
    assert_eq!(
        loaded["annotations"]["annotations"]["1"],
        annotations["annotations"]["1"]
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    store
        .export(
            &id,
            &json!({"recording_uuid":recording_id,"channel_id":"1","format":"yolo"}),
        )
        .unwrap();
    let project = store
        .add_named(&id, "datasets", &json!({"name":"Camera"}))
        .unwrap();
    let dataset = project["datasets"][0]["uuid"].as_str().unwrap();
    let project = store
        .add_dataset_entries(
            &id,
            dataset,
            &json!({"recording_uuid":recording_id,"channel_id":"1"}),
        )
        .unwrap();
    assert_eq!(project["datasets"][0]["entry_count"], 1);
    let mut invalid = loaded["annotations"].clone();
    invalid["techniques"]["2"] = json!("classification");
    assert!(store.save_annotations(&id, recording_id, &invalid).is_err());
}

#[tokio::test]
async fn recording_deletion_requires_dataset_cleanup_and_preserves_import_source() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(workspace.path()).unwrap());
    let (id, recording) = imported(&store, workspace.path());
    let recording_id = recording["uuid"].as_str().unwrap();
    store
        .add_named(&id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let source = workspace.path().join("sample.mcap");
    let original_hash = file_hash(&source).unwrap();
    let project = store.save_structured_dataset(&id, recording_id, "2", &json!({"name":"Motor values","columns":[{"id":"p","name":"position","source":"/position"}],"overrides":{}})).unwrap();
    let dataset_id = project["datasets"][0]["uuid"].as_str().unwrap();
    let folder = std::path::PathBuf::from(project["path"].as_str().unwrap());
    let managed = folder.join(format!("mcap/{recording_id}"));
    assert!(store
        .delete_recording(&id, recording_id)
        .unwrap_err()
        .to_string()
        .contains("Motor values"));
    assert!(managed.join("recording.mcap").is_file());
    let cache = workspace
        .path()
        .join(format!("data/cache/{id}/{recording_id}"));
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("probe"), "cached data").unwrap();
    store.delete_dataset(&id, dataset_id).unwrap();
    let response = router(store.clone())
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/projects/{id}/recordings/{recording_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!managed.exists());
    assert!(!cache.exists());
    assert_eq!(file_hash(&source).unwrap(), original_hash);
    let reloaded = Store::new(workspace.path()).unwrap();
    let project = reloaded.project(&id).unwrap();
    assert_eq!(project["mcaps"], json!([]));
    assert_eq!(project["labels"].as_array().unwrap().len(), 1);
    assert!(reloaded.recording(&id, recording_id).is_err());
}
