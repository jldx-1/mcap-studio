use mcap_workbench::Store;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

#[test]
fn reopening_old_unsupported_camera_caches_builds_playback_for_mono14_and_fp32() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path()).unwrap();
    let project = store
        .create_project(&json!({"name":"Camera formats"}))
        .unwrap();
    let source = directory.path().join("cameras.mcap");
    let mut writer = mcap::Writer::new(fs::File::create(&source).unwrap()).unwrap();
    for encoding in ["mono14", "FP32C1", "FP32C3"] {
        let channel = writer
            .add_channel(0, &format!("/{encoding}"), "json", &BTreeMap::new())
            .unwrap();
        for sequence in 0..2 {
            let data: Vec<u8> = if encoding == "mono14" {
                [0u16, 8191, 16383]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect()
            } else {
                let values = if sequence == 0 {
                    [0.25f32, 0.5, 0.75]
                } else {
                    [-2.0, 0.0, 2.0]
                };
                values.into_iter().flat_map(f32::to_le_bytes).collect()
            };
            let record = json!({"width":if encoding == "FP32C3" {1} else {3},"height":1,"encoding":encoding,"data":data});
            writer
                .write_to_known_channel(
                    &mcap::records::MessageHeader {
                        channel_id: channel,
                        sequence,
                        log_time: 100 + sequence as u64,
                        publish_time: 100 + sequence as u64,
                    },
                    &serde_json::to_vec(&record).unwrap(),
                )
                .unwrap();
        }
    }
    writer.finish().unwrap();
    drop(writer);
    let id = project["uuid"].as_str().unwrap();
    let recording = store.import_recording(id, "cameras.mcap", &source).unwrap();
    let recording_id = recording["uuid"].as_str().unwrap();
    let frames =
        Path::new(project["path"].as_str().unwrap()).join(format!("mcap/{recording_id}/frames"));
    let index_path = frames.join("index.json");
    let mut index: Value = serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
    index
        .as_object_mut()
        .unwrap()
        .remove("image_decoder_version");
    for topic in index["topics"].as_object_mut().unwrap().values_mut() {
        topic["unsupported"] = json!(true);
        topic["frames"] = json!([]);
        topic["errors"] = json!(["At 100: Unsupported raw image encoding old-format"]);
    }
    fs::write(&index_path, serde_json::to_vec(&index).unwrap()).unwrap();
    let reopened = store.recording(id, recording_id).unwrap();
    for topic in reopened["topics"].as_array().unwrap() {
        assert_eq!(topic["kind"], "image");
        assert_eq!(topic["frame_count"], 2);
        assert_ne!(topic["unsupported"], true);
        assert_eq!(topic["errors"], json!([]));
    }
    let index: Value = serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
    assert!(!mcap_workbench::engine::image_cache_needs_upgrade(&index));
    for topic in index["topics"].as_object().unwrap().values() {
        for (number, frame) in topic["frames"].as_array().unwrap().iter().enumerate() {
            let preview = frames.join(frame["image"].as_str().unwrap());
            let pixels = image::open(&preview).unwrap().to_rgb8().into_raw();
            let values = if topic["image_encoding"] == "mono14" {
                [0, 127, 255]
            } else if number == 0 {
                [63, 127, 191]
            } else {
                [0, 127, 255]
            };
            let expected: Vec<u8> = if topic["image_encoding"] == "FP32C3" {
                values.into_iter().rev().collect()
            } else {
                values.into_iter().flat_map(|v| [v; 3]).collect()
            };
            assert_eq!(pixels, expected);
            assert!(preview.with_extension("native.bin").is_file());
        }
    }
}
