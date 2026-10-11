//! Native engine regression fixtures and precision checks.
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::Path,
};

use super::*;

fn sample(time: u64, value: i64) -> Sample {
    Sample {
        ordinal: time as usize,
        time,
        log_time: time,
        publish_time: time,
        sequence: time as u32,
        record: json!({"value": value, "label": "original"}),
    }
}

#[test]
fn translations_copy_rename_and_arithmetic_without_execution() {
    let source = json!({"pose": {"x": 3, "y": 8}, "timestamp": 1_800_000_000_000_000_001_u64});
    let translated = translate_record(&source, "* -> *\npose.x -> position.x * 100\nclock <- source.timestamp + 1\nratio <- (source.pose.y - 2) / 3").unwrap();
    assert_eq!(translated["position"]["x"], 300);
    assert_eq!(translated["pose"], json!({"y": 8}));
    assert_eq!(translated["clock"], 1_800_000_000_000_000_002_u64);
    assert_eq!(translated["ratio"], 2.0);
    assert!(translate_record(&source, "value <- __import__(\"os\")").is_err());
    assert!(translate_record(&source, "value <- source.pose.x / 0").is_err());
}

#[test]
fn synchronization_nearest_linear_rate_and_tolerance() {
    let samples = vec![sample(10, 10), sample(20, 30), sample(30, 90)];
    let (nearest, _) = resample(&samples, 16, &json!({"mode":"nearest"}))
        .unwrap()
        .unwrap();
    assert_eq!(nearest.record["value"], 30);
    let (linear, _) = resample(&samples, 25, &json!({"mode":"linear"}))
        .unwrap()
        .unwrap();
    assert_eq!(linear.record["value"], 60);
    let (rate, _) = resample(&samples, 25, &json!({"mode":"rate"}))
        .unwrap()
        .unwrap();
    assert_eq!(rate.record["value"], 40);
    assert!(
        resample(&samples, 16, &json!({"mode":"nearest", "tolerance_ns":3}))
            .unwrap()
            .is_none()
    );
    assert!(resample(&samples, 16, &json!({"mode":"exact"}))
        .unwrap()
        .is_none());
    assert!(resample(&samples, 10, &json!({"mode":"unknown"})).is_err());
    assert!(resample(&samples, 10, &json!({"tolerance_ns":-1})).is_err());
}

#[test]
fn integer_interpolation_preserves_nanoseconds_and_uses_even_rounding() {
    assert_eq!(interpolate_integer(10, 11, (1, 2)).unwrap(), 10);
    assert_eq!(interpolate_integer(11, 12, (1, 2)).unwrap(), 12);
    assert_eq!(interpolate_integer(11, 12, (-1, 2)).unwrap(), 10);
    let time = 1_800_000_000_000_000_001_i128;
    assert_eq!(
        interpolate_integer(time, time + 2, (1, 2)).unwrap(),
        time as u64 + 1
    );
    assert_eq!(
        interpolate_integer(
            i64::MIN as i128,
            u64::MAX as i128,
            ((u64::MAX / 2) as i128, u64::MAX as i128)
        )
        .unwrap(),
        4_611_686_018_427_387_903_u64
    );
    assert!(interpolate_integer(0, u64::MAX as i128, (u64::MAX as i128, 1)).is_err());
}

#[test]
fn duplicate_source_clocks_last_wins_even_with_identical_sequence_numbers() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("duplicates.mcap");
    let mut writer = mcap::Writer::new(File::create(&source).unwrap()).unwrap();
    let parent = writer
        .add_channel(0, "/parent", "json", &BTreeMap::new())
        .unwrap();
    let child = writer
        .add_channel(0, "/child", "json", &BTreeMap::new())
        .unwrap();
    for (channel_id, value) in [(parent, 0), (child, 1), (child, 2)] {
        writer
            .write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id,
                    sequence: 0,
                    log_time: 10,
                    publish_time: 10,
                },
                &serde_json::to_vec(&json!({"value":value})).unwrap(),
            )
            .unwrap();
    }
    writer.finish().unwrap();
    drop(writer);
    let destination = directory.path().join("processed.mcap");
    let stats = process_recording(
        &source,
        &destination,
        &json!({"sync": {
            "enabled":true,"parent_channel":parent,"mode":"exact","delete_out_of_sync":true
        }}),
    )
    .unwrap();
    assert_eq!(stats["message_counts"]["misaligned"], 1);
    assert_eq!(stats["message_counts"]["deleted"], 1);
    let bytes = fs::read(destination).unwrap();
    let message = mcap::MessageStream::new(&bytes)
        .unwrap()
        .map(Result::unwrap)
        .find(|message| message.channel.topic == "/child")
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&message.data).unwrap()["value"],
        2
    );
}

#[test]
fn raw_images_respect_stride_color_order_and_endian() {
    let image = decode_image(
        &json!({"width":1,"height":2,"encoding":"bgr8","step":4,"data":[0,0,255,99,0,255,0,99]}),
    )
    .unwrap()
    .to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(image.get_pixel(0, 1).0, [0, 255, 0]);
    let mono = decode_image(
        &json!({"width":1,"height":1,"encoding":"mono16","is_bigendian":true,"data":[255,255]}),
    )
    .unwrap()
    .to_rgb8();
    assert_eq!(mono.get_pixel(0, 0).0, [255, 255, 255]);
    assert!(decode_image(&json!({"format":"h264","data":[]})).is_err());
}

#[test]
fn synchronized_output_drops_incomplete_or_retains_unused_source_samples() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mcap");
    let mut writer = mcap::Writer::new(File::create(&source).unwrap()).unwrap();
    let parent = writer
        .add_channel(0, "/parent", "json", &BTreeMap::new())
        .unwrap();
    let child = writer
        .add_channel(0, "/child", "json", &BTreeMap::new())
        .unwrap();
    for (id, times) in [(parent, vec![0, 10]), (child, vec![5, 10, 20])] {
        for time in times {
            writer
                .write_to_known_channel(
                    &mcap::records::MessageHeader {
                        channel_id: id,
                        sequence: time as u32,
                        log_time: time,
                        publish_time: time + 100,
                    },
                    &serde_json::to_vec(&json!({"value": time})).unwrap(),
                )
                .unwrap();
        }
    }
    writer.finish().unwrap();
    drop(writer);
    let dropped_path = directory.path().join("dropped.mcap");
    let result = process_recording(
        &source,
        &dropped_path,
        &json!({"sync": {
            "enabled": true, "parent_channel": parent, "mode":"exact", "delete_out_of_sync":true
        }}),
    )
    .unwrap();
    assert_eq!(result["output_messages"], 2);
    assert_eq!(result["dropped_frames"], 1);
    assert_eq!(result["message_counts"]["deleted"], 3);
    let kept_path = directory.path().join("kept.mcap");
    let result = process_recording(
        &source,
        &kept_path,
        &json!({"sync": {
            "enabled": true, "parent_channel":parent, "mode":"exact", "delete_out_of_sync":false
        }}),
    )
    .unwrap();
    assert_eq!(result["output_messages"], 5);
    assert_eq!(result["dropped_frames"], 0);
    let bytes = fs::read(kept_path).unwrap();
    let messages = mcap::MessageStream::new(&bytes)
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.log_time)
            .collect::<Vec<_>>(),
        vec![0, 5, 10, 10, 20]
    );
    assert!(messages
        .iter()
        .all(|message| message.publish_time == message.log_time + 100));
}

#[test]
fn protobuf_fixture_decodes_and_processed_mcap_round_trips() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../utils/synthetic_robot_arm.mcap");
    let bytes = fs::read(&fixture).unwrap();
    let mut decoder = Decoder::default();
    let mut counts = BTreeMap::new();
    let mut camera_checked = false;
    for message in mcap::MessageStream::new(&bytes).unwrap() {
        let message = message.unwrap();
        let record = decoder.decode(&message).unwrap();
        *counts.entry(message.channel.id).or_insert(0) += 1;
        if message.channel.topic == "/camera/rgb" && !camera_checked {
            let image = decode_image(&record).unwrap();
            assert_eq!((image.width(), image.height()), (320, 240));
            camera_checked = true;
        }
    }
    assert_eq!(counts.len(), 8);
    assert!(camera_checked);
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("processed.mcap");
    let result = process_recording(&fixture, &destination, &json!({"selected_channels":["8"], "translations":{"8":"* -> *\nx_m -> position.x * 1000"},"sync":{"enabled":false}})).unwrap();
    assert_eq!(result["output_messages"], counts[&8]);
    let output = fs::read(&destination).unwrap();
    let messages = mcap::MessageStream::new(&output)
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    let record: Value = serde_json::from_slice(&messages[0].data).unwrap();
    assert!(record["position"]["x"].is_number());
    assert!(record.get("x_m").is_none());
    assert_eq!(messages[0].channel.message_encoding, "json");
    let index = build_index(&destination, directory.path(), "test-sha").unwrap();
    let topic = index["topics"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(topic["frames"].as_array().unwrap().len(), counts[&8]);
    assert!(topic["errors"].as_array().unwrap().is_empty());
}
use super::images::decode_image;
use super::synchronization::{interpolate_integer, resample, Sample};

#[test]
fn training_depth_tensors_preserve_values_stride_and_endianness() {
    let folder = tempfile::tempdir().unwrap();
    let preview = folder.path().join("depth.png");
    let record = json!({"width":2,"height":2,"step":6,"encoding":"16UC1","is_bigendian":true,"data":[3,232,7,208,99,99,11,184,15,160,99,99]});
    assert!(super::training_images::preserve_native(&record, &preview, true).unwrap());
    let path = folder.path().join("depth.npy");
    let metadata = super::write_training_tensor(&preview, &path, true)
        .unwrap()
        .unwrap();
    assert_eq!(metadata["scale"], 1.0);
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..8], b"\x93NUMPY\x01\x00");
    let header = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let values: Vec<_> = bytes[10 + header..]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    assert_eq!(values, vec![1000, 2000, 3000, 4000]);
}

#[test]
fn float_depth_training_values_are_not_normalized_to_preview_pixels() {
    let folder = tempfile::tempdir().unwrap();
    let preview = folder.path().join("depth.png");
    let values = [0.125f32, 2.5, 4.75, 12.0];
    let data: Vec<_> = values.into_iter().flat_map(f32::to_le_bytes).collect();
    let record = json!({"width":2,"height":2,"step":8,"encoding":"32FC1","data":data});
    super::training_images::preserve_native(&record, &preview, true).unwrap();
    let path = folder.path().join("depth.npy");
    let metadata = super::write_training_tensor(&preview, &path, true)
        .unwrap()
        .unwrap();
    assert_eq!(metadata["dtype"], "<f4");
    let bytes = std::fs::read(path).unwrap();
    let header = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let decoded: Vec<_> = bytes[10 + header..]
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(decoded, values);
}
