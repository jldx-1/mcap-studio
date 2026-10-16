use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use mcap_workbench::{accounts::Principal, router, Store};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use tower::ServiceExt;

fn workspace() -> (tempfile::TempDir, Arc<Store>, Principal, String, Value) {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(directory.path()).unwrap());
    let (actor, token) = store
        .accounts
        .setup(&json!({"username":"admin","name":"Administrator","password":"test-password-123"}))
        .unwrap();
    let project = store
        .create_project(&json!({"name":"Import tests"}))
        .unwrap();
    (directory, store, actor, token, project)
}

fn png() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(20, 10, image::Rgb([255, 0, 0])))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn files(directory: &Path, rows: &[(&str, Vec<u8>)]) -> Vec<(String, PathBuf)> {
    rows.iter()
        .enumerate()
        .map(|(i, (name, bytes))| {
            let path = directory.join(format!("upload-{i}"));
            fs::write(&path, bytes).unwrap();
            (name.to_string(), path)
        })
        .collect()
}

fn zip_file(directory: &Path, rows: &[(&str, Vec<u8>)]) -> Vec<(String, PathBuf)> {
    let path = directory.join("dataset.zip");
    let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    for (name, bytes) in rows {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
    vec![("dataset.zip".into(), path)]
}

#[test]
fn yolo_import_preserves_splits_labels_frames_and_class_names_across_reload_and_export() {
    let (directory, store, actor, _, project) = workspace();
    let id = project["uuid"].as_str().unwrap();
    let rows = [
        ("set/images/train/frame.png", png()),
        (
            "set/labels/train/frame.txt",
            b"0 0.5 0.5 0.4 0.6\n".to_vec(),
        ),
        ("set/images/val/frame.png", png()),
        ("set/labels/val/frame.txt", b"1 0.5 0.5 1 1\n".to_vec()),
        ("set/images/train/negative.png", png()),
        (
            "set/data.yaml",
            b"names:\n  0: robot\n  1: person\n".to_vec(),
        ),
        ("set/README.txt", b"Dataset documentation".to_vec()),
    ];
    let uploaded = zip_file(directory.path(), &rows);
    let result = store
        .import_yolo(id, "Imported objects", &uploaded, &[], &actor)
        .unwrap();
    let dataset = &result["dataset"];
    let dataset_id = dataset["uuid"].as_str().unwrap();
    assert_eq!(dataset["kind"], "imported-yolo");
    assert_eq!(dataset["entry_count"], 3);
    assert_eq!(dataset["class_names"], json!(["robot", "person"]));
    assert_eq!(dataset["split_counts"], json!({"train":2,"val":1}));
    assert_eq!(dataset["box_count"], 2);
    assert_eq!(dataset["negative_count"], 1);
    let restored = Store::new(directory.path()).unwrap();
    assert_eq!(restored.dataset(id, dataset_id).unwrap()["entry_count"], 3);
    let samples = restored.yolo_samples(id, dataset_id, 0, 100).unwrap();
    assert_eq!(samples["samples"].as_array().unwrap().len(), 3);
    let val = samples["samples"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sample| sample["split"] == "val")
        .unwrap();
    assert_eq!(val["boxes"][0]["class_id"], 1);
    assert_eq!(val["width"], 20);
    assert_eq!(val["height"], 10);
    assert_eq!(
        restored
            .yolo_image(id, dataset_id, val["index"].as_u64().unwrap() as usize)
            .unwrap()
            .0,
        png()
    );
    assert!(restored.yolo_image(id, dataset_id, 1000).is_err());
    assert_eq!(
        restored.yolo_samples(id, dataset_id, 2, 1).unwrap()["samples"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let archive = restored.yolo_archive(id, dataset_id).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut config = String::new();
    archive
        .by_name("data.yaml")
        .unwrap()
        .read_to_string(&mut config)
        .unwrap();
    let config: Value = serde_yaml_ng::from_str(&config).unwrap();
    assert_eq!(config["names"], json!(["robot", "person"]));
    assert_eq!(config["val"], "images/val");
    assert_eq!(archive.len(), 8); // three images, three labels, YAML, class list
}

#[test]
fn yolo_validation_is_atomic_and_rejects_orphans_bad_boxes_duplicates_and_traversal() {
    let (directory, store, actor, _, project) = workspace();
    let id = project["uuid"].as_str().unwrap();
    for label in [
        "0 2 0.5 0.2 0.2",
        "0 NaN 0.5 0.2 0.2",
        "-1 0.5 0.5 0.2 0.2",
        "0 0.5 0.5 0.2 0.2 0.7",
    ] {
        let uploaded = files(
            directory.path(),
            &[
                ("image.png", png()),
                ("image.txt", label.as_bytes().to_vec()),
            ],
        );
        assert!(store
            .import_yolo(id, "Invalid", &uploaded, &[], &actor)
            .is_err());
        assert!(store.project(id).unwrap()["datasets"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    let orphan = files(
        directory.path(),
        &[
            ("image.png", png()),
            ("labels/missing.txt", b"0 0.5 0.5 1 1".to_vec()),
        ],
    );
    assert!(store
        .import_yolo(id, "Orphan", &orphan, &[], &actor)
        .unwrap_err()
        .to_string()
        .contains("corresponding image"));
    let duplicate = files(
        directory.path(),
        &[("image.png", png()), ("image.jpg", png())],
    );
    assert!(store
        .import_yolo(id, "Duplicate", &duplicate, &[], &actor)
        .is_err());
    let unsafe_zip = zip_file(directory.path(), &[("../outside.png", png())]);
    assert!(store
        .import_yolo(id, "Unsafe", &unsafe_zip, &[], &actor)
        .is_err());
    assert!(!directory.path().join("outside.png").exists());
    let valid = files(
        directory.path(),
        &[
            ("image.png", png()),
            ("image.txt", b"1 0.5 0.5 1 1".to_vec()),
        ],
    );
    assert!(store
        .import_yolo(id, "Class mismatch", &valid, &["robot".into()], &actor)
        .is_err());
    store.accounts.save_user(&actor,None,&json!({"username":"viewer","name":"Viewer","password":"test-password-123","permissions":[],"groups":[]})).unwrap();
    let (viewer, _) = store
        .accounts
        .login(&json!({"username":"viewer","password":"test-password-123"}))
        .unwrap();
    assert!(store
        .import_yolo(id, "Denied", &valid, &[], &viewer)
        .is_err());
    assert!(store.project(id).unwrap()["datasets"]
        .as_array()
        .unwrap()
        .is_empty());
}

fn recording(path: &Path, topic: &str, schema: &[u8]) {
    let mut writer = mcap::Writer::new(fs::File::create(path).unwrap()).unwrap();
    let schema = writer.add_schema("Motor", "jsonschema", schema).unwrap();
    let channel = writer
        .add_channel(schema, topic, "json", &BTreeMap::new())
        .unwrap();
    for sequence in 0..2 {
        writer
            .write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id: channel,
                    sequence,
                    log_time: 100 + sequence as u64,
                    publish_time: 100 + sequence as u64,
                },
                &serde_json::to_vec(&json!({"value":3+sequence,"values":[1,2,9]})).unwrap(),
            )
            .unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn translation_files_match_topics_and_schema_versions_and_apply_to_a_separate_recording() {
    let (directory, store, actor, _, project) = workspace();
    let id = project["uuid"].as_str().unwrap();
    let source = directory.path().join("source.mcap");
    recording(&source, "/motor", b"{\"type\":\"object\"}");
    let imported = store.import_recording(id, "source.mcap", &source).unwrap();
    let recording_id = imported["uuid"].as_str().unwrap();
    let metadata = store.topic_translations(id, recording_id).unwrap();
    let topic = &metadata["topics"][0];
    let channel = topic["channel_id"].as_str().unwrap();
    let profile=store.save_topic_translation(id,recording_id,&json!({"channel_id":channel,"translation":{"format_version":1,"name":"Motor training values","source_message":topic["source_message"],"source_schema_sha256":topic["source_schema_sha256"],"output_type":"TrainingRecord","mappings":[{"target":"value_mm","expression":"source.value * 1000"},{"target":"last","expression":"max(source.values)"}]}}),&actor).unwrap();
    let preview = store
        .preview_topic_translation(
            id,
            recording_id,
            &json!({"channel_id":channel,"script":profile["script"]}),
        )
        .unwrap();
    assert_eq!(preview["translated"], json!({"value_mm":3000,"last":9}));
    for translation in [
        json!({"name":"Invalid output type","script":"* -> *","output_type":""}),
        json!({"name":"Requires Protobuf","script":"* -> *","output_schema":{"descriptor_set_base64":"AA=="}}),
    ] {
        assert!(store
            .save_topic_translation(
                id,
                recording_id,
                &json!({"channel_id":channel,"translation":translation}),
                &actor
            )
            .is_err());
    }
    let second = store.import_recording(id, "again.mcap", &source).unwrap();
    assert_eq!(
        store
            .topic_translations(id, second["uuid"].as_str().unwrap())
            .unwrap()["topics"][0]["choices"][0]["id"],
        profile["id"]
    );
    let other = directory.path().join("other.mcap");
    recording(&other, "/other", b"{\"type\":\"object\"}");
    let other = store.import_recording(id, "other.mcap", &other).unwrap();
    assert!(store
        .topic_translations(id, other["uuid"].as_str().unwrap())
        .unwrap()["topics"][0]["choices"]
        .as_array()
        .unwrap()
        .is_empty());
    let changed = directory.path().join("changed.mcap");
    recording(
        &changed,
        "/motor",
        b"{\"type\":\"object\",\"properties\":{\"value\":{\"type\":\"number\"}}}",
    );
    let changed = store
        .import_recording(id, "changed.mcap", &changed)
        .unwrap();
    assert!(store
        .topic_translations(id, changed["uuid"].as_str().unwrap())
        .unwrap()["topics"][0]["choices"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(store
        .save_topic_translation(
            id,
            changed["uuid"].as_str().unwrap(),
            &json!({"channel_id":channel,"translation":profile}),
            &actor
        )
        .is_err());
    assert!(store
        .apply_topic_translations(
            id,
            changed["uuid"].as_str().unwrap(),
            &json!({"selections":{channel:profile["id"]}})
        )
        .is_err());
    let processed = store
        .apply_topic_translations(
            id,
            recording_id,
            &json!({"selections":{channel:profile["id"]}}),
        )
        .unwrap();
    assert_ne!(processed["uuid"], imported["uuid"]);
    let processed_channel = processed["topics"][0]["channel_id"].as_str().unwrap();
    assert_eq!(
        store
            .record(
                id,
                processed["uuid"].as_str().unwrap(),
                processed_channel,
                0
            )
            .unwrap(),
        json!({"value_mm":3000,"last":9})
    );
    assert_eq!(
        store.record(id, recording_id, channel, 0).unwrap()["value"],
        3
    );
    assert_eq!(
        fs::read(&source).unwrap(),
        fs::read(
            Path::new(project["path"].as_str().unwrap()).join(imported["path"].as_str().unwrap())
        )
        .unwrap()
    );
}

#[tokio::test]
async fn multipart_yolo_routes_publish_samples_previews_and_downloads() {
    let (_directory, store, _actor, token, project) = workspace();
    let id = project["uuid"].as_str().unwrap();
    let boundary = "mcap-yolo-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nHTTP dataset\r\n"
    )
    .into_bytes();
    for (name, bytes) in [
        ("folder/images/train/image.png", png()),
        ("folder/labels/train/image.txt", b"0 0.5 0.5 1 1".to_vec()),
        ("folder/classes.txt", b"robot\n".to_vec()),
    ] {
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes());
        body.extend(bytes);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{boundary}--\r\n").into_bytes());
    let app = router(store);
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/projects/{id}/datasets/import-yolo"))
                .header("cookie", format!("mcap_session={token}"))
                .header("x-requested-with", "mcap-workbench")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    let dataset = result["dataset"]["uuid"].as_str().unwrap();
    for suffix in ["samples?offset=0&limit=1", "images/0", "archive"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/api/projects/{id}/datasets/{dataset}/{suffix}"))
                    .header("cookie", format!("mcap_session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        if suffix.starts_with("samples") {
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let samples: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                samples["samples"][0]["source_file"],
                "folder/images/train/image.png"
            );
            assert_eq!(samples["samples"][0]["boxes"][0]["class_id"], 0);
        }
    }
    let response = app
        .oneshot(
            Request::get(format!("/api/projects/{id}/datasets/{dataset}/samples"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn selected_translation_can_extract_fields_from_an_unsupported_camera_encoding() {
    let (directory, store, actor, _, project) = workspace();
    let id = project["uuid"].as_str().unwrap();
    let source = directory.path().join("camera.mcap");
    let mut writer = mcap::Writer::new(fs::File::create(&source).unwrap()).unwrap();
    let channel = writer
        .add_channel(0, "/camera/raw", "json", &BTreeMap::new())
        .unwrap();
    writer
        .write_to_known_channel(
            &mcap::records::MessageHeader {
                channel_id: channel,
                sequence: 0,
                log_time: 100,
                publish_time: 100,
            },
            &serde_json::to_vec(
                &json!({"width":1,"height":1,"encoding":"bayer_rggb8","data":[255]}),
            )
            .unwrap(),
        )
        .unwrap();
    writer.finish().unwrap();
    drop(writer);
    let imported = store.import_recording(id, "camera.mcap", &source).unwrap();
    assert_eq!(imported["topics"][0]["unsupported"], true);
    let recording = imported["uuid"].as_str().unwrap();
    let channel = imported["topics"][0]["channel_id"].as_str().unwrap();
    let translation = store.save_topic_translation(id,recording,&json!({"channel_id":channel,"translation":{"name":"Camera dimensions","script":"width_px <- source.width"}}),&actor).unwrap();
    let processed = store
        .apply_topic_translations(
            id,
            recording,
            &json!({"selections":{channel:translation["id"]}}),
        )
        .unwrap();
    assert_eq!(processed["topics"][0]["frame_count"], 1);
    assert_eq!(processed["topics"][0]["kind"], "record");
    let output_channel = processed["topics"][0]["channel_id"].as_str().unwrap();
    assert_eq!(
        store
            .record(id, processed["uuid"].as_str().unwrap(), output_channel, 0)
            .unwrap(),
        json!({"width_px":1})
    );
}

#[tokio::test]
async fn mcap_upload_reuses_existing_sources_and_applies_known_presets() {
    let (directory, store, actor, token, project) = workspace();
    let id = project["uuid"].as_str().unwrap();
    let source = directory.path().join("source.mcap");
    recording(&source, "/motor", b"{\"type\":\"object\"}");
    let original = store
        .import_recording(id, "reference.mcap", &source)
        .unwrap();
    let channel = original["topics"][0]["channel_id"].as_str().unwrap();
    let profile=store.save_topic_translation(id,original["uuid"].as_str().unwrap(),&json!({"channel_id":channel,"translation":{"name":"Motor conversion","script":"value_mm <- source.value * 1000"}}),&actor).unwrap();
    let app = router(store.clone());
    let upload = |query: &str| {
        let boundary = "automatic-translation-boundary";
        let mut body=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"next.mcap\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
        body.extend(fs::read(&source).unwrap());
        body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
        Request::post(format!(
            "/api/projects/{id}/recordings?defer_index=true{query}"
        ))
        .header("cookie", format!("mcap_session={token}"))
        .header("x-requested-with", "mcap-workbench")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
    };
    let response = app.clone().oneshot(upload("")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(result["translated_topics"], 1);
    let source_id = result["source_recording"]["uuid"].as_str().unwrap();
    let output_id = result["uuid"].as_str().unwrap();
    assert_ne!(source_id, output_id);
    let folder = Path::new(project["path"].as_str().unwrap());
    assert_eq!(result["source_recording"]["duplicate"], true);
    assert_eq!(result["source_recording"]["uuid"], original["uuid"]);
    assert_eq!(
        fs::read(folder.join(result["source_recording"]["path"].as_str().unwrap())).unwrap(),
        fs::read(&source).unwrap()
    );
    let output_channel = result["topics"][0]["channel_id"].as_str().unwrap();
    assert_eq!(
        store.record(id, output_id, output_channel, 0).unwrap(),
        json!({"value_mm":3000})
    );
    let repeated = store
        .apply_topic_translations(
            id,
            source_id,
            &json!({"selections":{channel:profile["id"]}}),
        )
        .unwrap();
    assert_eq!(repeated["uuid"], result["uuid"]);
    assert_eq!(
        store.project(id).unwrap()["mcaps"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Updating an existing preset invalidates reuse even when its ID stays the same.
    let changed = store.save_topic_translation(id,original["uuid"].as_str().unwrap(),&json!({"channel_id":channel,"translation":{"name":"Motor conversion","script":"value_mm <- source.value * 2000"}}),&actor).unwrap();
    assert_eq!(changed["id"], profile["id"]);
    let changed_output = store
        .apply_topic_translations(
            id,
            source_id,
            &json!({"selections":{channel:changed["id"]}}),
        )
        .unwrap();
    assert_ne!(changed_output["uuid"], result["uuid"]);
    assert_eq!(
        store
            .record(
                id,
                changed_output["uuid"].as_str().unwrap(),
                changed_output["topics"][0]["channel_id"].as_str().unwrap(),
                0
            )
            .unwrap()["value_mm"],
        6000
    );
    // Explicitly bypass presets for a raw import.
    let response = app
        .clone()
        .oneshot(upload("&auto_translate=false"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let raw: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(raw["duplicate"], true);
    assert_eq!(raw["uuid"], original["uuid"]);
    assert!(raw.get("source_recording").is_none());
    // Multiple matching presets require a choice instead of silently choosing a script.
    store.save_topic_translation(id,original["uuid"].as_str().unwrap(),&json!({"channel_id":channel,"translation":{"name":"Alternative conversion","script":"value_cm <- source.value * 100"}}),&actor).unwrap();
    let response = app.oneshot(upload("")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let ambiguous: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(ambiguous.get("source_recording").is_none());
    assert!(ambiguous["translation_warning"]
        .as_str()
        .unwrap()
        .contains("Choose a translation manually"));
}

#[test]
fn automatic_translation_failure_keeps_the_import_and_no_partial_output() {
    let (directory, store, actor, _, project) = workspace();
    let id = project["uuid"].as_str().unwrap();
    let source = directory.path().join("source.mcap");
    recording(&source, "/motor", b"{\"type\":\"object\"}");
    let reference = store
        .import_recording(id, "reference.mcap", &source)
        .unwrap();
    let channel = reference["topics"][0]["channel_id"].as_str().unwrap();
    store.save_topic_translation(id,reference["uuid"].as_str().unwrap(),&json!({"channel_id":channel,"translation":{"name":"Scale","script":"scaled <- source.value * 2"}}),&actor).unwrap();
    let failing = directory.path().join("failing.mcap");
    let mut writer = mcap::Writer::new(fs::File::create(&failing).unwrap()).unwrap();
    let schema = writer
        .add_schema("Motor", "jsonschema", b"{\"type\":\"object\"}")
        .unwrap();
    let channel = writer
        .add_channel(schema, "/motor", "json", &BTreeMap::new())
        .unwrap();
    for (sequence, data) in [
        b"{\"value\":3}".as_slice(),
        b"{\"value\":\"bad\"}".as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        writer
            .write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id: channel,
                    sequence: sequence as u32,
                    log_time: sequence as u64,
                    publish_time: sequence as u64,
                },
                data,
            )
            .unwrap();
    }
    writer.finish().unwrap();
    drop(writer);
    let imported = store
        .import_recording(id, "failing.mcap", &failing)
        .unwrap();
    let result = store.auto_translate_import(id, imported.clone());
    assert_eq!(result["uuid"], imported["uuid"]);
    assert!(result["translation_warning"]
        .as_str()
        .unwrap()
        .contains("could not be applied"));
    assert_eq!(
        store.project(id).unwrap()["mcaps"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let folder = Path::new(project["path"].as_str().unwrap()).join("mcap");
    assert_eq!(fs::read_dir(folder).unwrap().count(), 2);
}

/// Exercise the real multipart extractor beyond its former 2 GiB cap. A repeated
/// ignored form field keeps both the fixture allocation and disk usage small.
#[tokio::test]
#[ignore = "streams over 2 GiB; run explicitly for large-upload regression"]
async fn mcap_upload_streams_past_two_gib() {
    use axum::body::{Bytes, HttpBody};
    use http_body::{Frame, SizeHint};
    use std::{
        collections::VecDeque,
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };

    struct Upload {
        prefix: Option<Bytes>,
        padding: Bytes,
        remaining: usize,
        suffix: VecDeque<Bytes>,
    }
    impl HttpBody for Upload {
        type Data = Bytes;
        type Error = Infallible;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
            let bytes = if let Some(prefix) = self.prefix.take() {
                Some(prefix)
            } else if self.remaining > 0 {
                self.remaining -= 1;
                Some(self.padding.clone())
            } else {
                self.suffix.pop_front()
            };
            Poll::Ready(bytes.map(|bytes| Ok(Frame::data(bytes))))
        }
        fn size_hint(&self) -> SizeHint {
            let size = self.prefix.as_ref().map_or(0, Bytes::len)
                + self.padding.len() * self.remaining
                + self.suffix.iter().map(Bytes::len).sum::<usize>();
            SizeHint::with_exact(size as u64)
        }
    }
    let (directory, store, _, token, project) = workspace();
    let source = directory.path().join("source.mcap");
    recording(&source, "/motor", b"{\"type\":\"object\"}");
    let original = fs::read(source).unwrap();
    let body = Upload {
        prefix: Some(Bytes::from_static(b"--large-boundary\r\nContent-Disposition: form-data; name=\"padding\"\r\n\r\n")),
        padding: Bytes::from(vec![b'x'; 1024 * 1024]),
        remaining: 2049,
        suffix: VecDeque::from([
            Bytes::from_static(b"\r\n--large-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"large.mcap\"\r\nContent-Type: application/octet-stream\r\n\r\n"),
            Bytes::from(original.clone()),
            Bytes::from_static(b"\r\n--large-boundary--\r\n"),
        ]),
    };
    let response = router(store)
        .oneshot(
            Request::post(format!(
                "/api/projects/{}/recordings?defer_index=true&auto_translate=false",
                project["uuid"].as_str().unwrap()
            ))
            .header("cookie", format!("mcap_session={token}"))
            .header("x-requested-with", "mcap-workbench")
            .header(
                "content-type",
                "multipart/form-data; boundary=large-boundary",
            )
            .body(Body::new(body))
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let imported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(imported["indexing"], "pending");
    let path =
        Path::new(project["path"].as_str().unwrap()).join(imported["path"].as_str().unwrap());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[tokio::test]
async fn upload_worker_handles_large_chunks_and_cleans_up_incomplete_transfers() {
    let (dir, store, _, token, project) = workspace();
    let id = project["uuid"].as_str().unwrap();
    let source = dir.path().join("large.mcap");
    let mut writer = mcap::WriteOptions::default()
        .compression(None)
        .create(fs::File::create(&source).unwrap())
        .unwrap();
    let channel = writer
        .add_channel(0, "/opaque", "custom", &BTreeMap::new())
        .unwrap();
    writer
        .write_to_known_channel(
            &mcap::records::MessageHeader {
                channel_id: channel,
                sequence: 0,
                log_time: 1,
                publish_time: 1,
            },
            &vec![42; 5 * 1024 * 1024],
        )
        .unwrap();
    writer.finish().unwrap();
    drop(writer);
    let original = fs::read(&source).unwrap();
    let upload = |complete| {
        let mut bytes = b"--chunks\r\nContent-Disposition: form-data; name=\"file\"; filename=\"large.mcap\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
        bytes.extend(&original);
        if complete {
            bytes.extend(b"\r\n--chunks--\r\n");
        }
        Request::post(format!(
            "/api/projects/{id}/recordings?defer_index=true&auto_translate=false"
        ))
        .header("cookie", format!("mcap_session={token}"))
        .header("x-requested-with", "mcap-workbench")
        .header("content-type", "multipart/form-data; boundary=chunks")
        .body(Body::from(bytes))
        .unwrap()
    };
    let app = router(store.clone());
    let response = app.clone().oneshot(upload(true)).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let saved: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        saved["sha256"],
        mcap_workbench::storage::file_hash(&source).unwrap()
    );
    let folder = Path::new(project["path"].as_str().unwrap());
    assert_eq!(
        fs::read(folder.join(saved["path"].as_str().unwrap())).unwrap(),
        original
    );
    let failed = app.oneshot(upload(false)).await.unwrap();
    assert_eq!(failed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        store.project(id).unwrap()["mcaps"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(fs::read_dir(folder.join("mcap"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".upload-")));
}

#[tokio::test]
async fn upload_limits_endpoint_exposes_the_server_setting_to_authenticated_uploaders() {
    let (_dir, store, _, token, project) = workspace();
    let path = format!(
        "/api/projects/{}/upload-limits",
        project["uuid"].as_str().unwrap()
    );
    let app = router(store);
    let denied = app
        .clone()
        .oneshot(Request::get(&path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(
            Request::get(&path)
                .header("cookie", format!("mcap_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let limits: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(limits["max_mcap_bytes"].is_null() || limits["max_mcap_bytes"].is_u64());
    assert!(limits.as_object().unwrap().contains_key("max_mcap_bytes"));
}
