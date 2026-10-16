//! Native engine regression fixtures and precision checks.
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::Path,
};

use super::*;

#[test]
fn translation_fingerprint_matches_legacy_canonical_descriptor_order() {
    use prost::Message;
    use sha2::{Digest, Sha256};
    use std::{borrow::Cow, sync::Arc};
    let first = prost_types::FileDescriptorProto {
        name: Some("a.proto".into()),
        syntax: Some("proto3".into()),
        ..Default::default()
    };
    let second = prost_types::FileDescriptorProto {
        name: Some("b.proto".into()),
        syntax: Some("proto3".into()),
        ..Default::default()
    };
    let canonical = prost_types::FileDescriptorSet {
        file: vec![first.clone(), second.clone()],
    }
    .encode_to_vec();
    let unsorted = prost_types::FileDescriptorSet {
        file: vec![second, first],
    }
    .encode_to_vec();
    let channel = mcap::Channel {
        id: 1,
        topic: "/known".into(),
        message_encoding: "protobuf".into(),
        metadata: BTreeMap::new(),
        schema: Some(Arc::new(mcap::Schema {
            id: 1,
            name: "Known".into(),
            encoding: "protobuf".into(),
            data: Cow::Owned(unsorted),
        })),
    };
    assert_eq!(
        super::translation_files::schema_fingerprint(&channel).unwrap(),
        format!("{:x}", Sha256::digest(canonical))
    );
}

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
fn legacy_translation_expressions_support_indexing_functions_and_constants() {
    let source = json!({"values":[3,-4,9],"entities":[{"position":{"x":0.125}}],"map":{"motor":12,"1":20},"clock":1_800_000_000_000_000_001_u64});
    let translated = translate_record(&source,"x_mm <- source.entities[0].position.x * 1000\nlast <- source.values[-1]\nabsolute <- abs(source.values[1])\nroot <- sqrt(max(source.values))\nsmallest <- min(source.values)\nrounded <- round(source.entities[0].position.x, 2)\nkey <- source.map['motor']\nnumber_key <- source.map[1]\nkeep <- True\nmissing <- None\nclock <- max(source.clock, source.clock - 1)").unwrap();
    assert_eq!(translated["x_mm"], 125.0);
    assert_eq!(translated["last"], 9);
    assert_eq!(translated["absolute"], 4);
    assert_eq!(translated["root"], 3.0);
    assert_eq!(translated["smallest"], -4);
    assert_eq!(translated["rounded"], 0.12);
    assert_eq!(translated["key"], 12);
    assert_eq!(translated["number_key"], 20);
    assert_eq!(translated["keep"], true);
    assert_eq!(translated["missing"], Value::Null);
    assert_eq!(translated["clock"], source["clock"]);
    assert_eq!(translate_record(&source,"clock <- round(source.clock, 2)\nlast_digits <- round(source.clock, -2)\ndecimal <- round(2.675, 2)\nnegative <- round(-45, -1)").unwrap(),json!({"clock":1_800_000_000_000_000_001_u64,"last_digits":1_800_000_000_000_000_000_u64,"decimal":2.67,"negative":-40}));
    assert_eq!(
        translate_record(&json!({}), "x <- round(2.5)\ny <- round(3.5)").unwrap(),
        json!({"x":2,"y":4})
    );
    for script in [
        "x <- source.values[100]",
        "x <- sqrt(-1)",
        "x <- round(1, 10000)",
        "x <- __import__('os')",
        "x <- source.values.__class__()",
        "x <- source.values[0:2]",
    ] {
        assert!(
            translate_record(&source, script).is_err(),
            "Accepted {script}"
        );
    }
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

fn float_image_record(
    encoding: &str,
    width: usize,
    height: usize,
    values: &[f32],
    big_endian: bool,
    padding: usize,
) -> Value {
    let samples_per_row = values.len() / height;
    let mut data = Vec::new();
    for row in values.chunks_exact(samples_per_row) {
        for value in row {
            data.extend(if big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            });
        }
        // Padding must not participate in normalization, even as float bytes.
        data.extend(vec![255; padding]);
    }
    json!({"width":width,"height":height,"encoding":encoding,
        "step":samples_per_row * 4 + padding,"is_bigendian":big_endian,"data":data})
}

#[test]
fn normalized_float_previews_keep_intensities_color_order_stride_and_endianness() {
    for big_endian in [false, true] {
        for encoding in ["32FC1", "32fc1", "mono32f"] {
            let image = decode_image(&float_image_record(
                encoding,
                2,
                2,
                &[0.25, 0.5, 0.75, 0.5],
                big_endian,
                3,
            ))
            .unwrap()
            .to_rgb8();
            assert_eq!(
                image.into_raw(),
                [63, 63, 63, 127, 127, 127, 191, 191, 191, 127, 127, 127]
            );
        }
        for encoding in ["32FC3", "32fc3", "bgr32f", "rgb32f"] {
            let values = if encoding == "rgb32f" {
                [0.75, 0.5, 0.25, 0.0, 0.5, 1.0]
            } else {
                [0.25, 0.5, 0.75, 1.0, 0.5, 0.0]
            };
            let mut record = float_image_record(encoding, 1, 2, &values, big_endian, 3);
            // Both metadata fields are used by supported camera schemas.
            record["pixel_format"] = record["encoding"].take();
            record["is_bigendian"] = json!(u8::from(big_endian));
            let image = decode_image(&record).unwrap().to_rgb8();
            assert_eq!(image.into_raw(), [191, 127, 63, 0, 127, 255]);
        }
    }
}

#[test]
fn float_previews_scale_all_channels_together_and_ignore_nonfinite_samples() {
    let color = decode_image(&float_image_record(
        "32FC3",
        2,
        1,
        &[2.0, 4.0, -2.0, 0.0, 1.0, 3.0],
        false,
        4,
    ))
    .unwrap()
    .to_rgb8();
    assert_eq!(color.into_raw(), [0, 255, 170, 212, 127, 85]);
    for encoding in ["32FC1", "32FC3"] {
        let channels = if encoding == "32FC1" { 1 } else { 3 };
        for (values, expected) in [
            (vec![0.5; channels * 2], vec![127; 6]),
            (vec![42.0; channels * 2], vec![0; 6]),
            (vec![f32::NAN; channels * 2], vec![0; 6]),
            (vec![f32::INFINITY; channels * 2], vec![0; 6]),
            (vec![f32::NEG_INFINITY; channels * 2], vec![0; 6]),
            (
                [vec![f32::NAN; channels], vec![1.0; channels]].concat(),
                vec![0, 0, 0, 255, 255, 255],
            ),
        ] {
            let image = decode_image(&float_image_record(encoding, 2, 1, &values, false, 0))
                .unwrap()
                .to_rgb8();
            assert_eq!(image.into_raw(), expected);
        }
    }
}

#[test]
fn float_previews_reject_short_payloads_and_short_strides() {
    for encoding in ["32FC1", "32FC3"] {
        let channels = if encoding == "32FC1" { 1 } else { 3 };
        let record = float_image_record(encoding, 1, 1, &vec![0.5; channels], false, 0);
        let mut short_stride = record.clone();
        short_stride["step"] = json!(channels * 4 - 1);
        assert!(decode_image(&short_stride).is_err());
        let mut short_payload = record;
        short_payload["data"].as_array_mut().unwrap().pop();
        assert!(decode_image(&short_payload).is_err());
    }
}

#[test]
fn float_camera_streams_build_playback_previews_and_preserve_training_tensors() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("float-cameras.mcap");
    let mut writer = mcap::Writer::new(File::create(&source).unwrap()).unwrap();
    let encodings = ["32FC1", "32FC3", "rgb32f", "bgr32f"];
    for encoding in encodings {
        let channel = writer
            .add_channel(0, &format!("/camera/{encoding}"), "json", &BTreeMap::new())
            .unwrap();
        for (sequence, values) in [(0, [0.25, 0.5, 0.75]), (1, [-2.0, 0.0, 2.0])] {
            let width = if encoding == "32FC1" { 3 } else { 1 };
            let record = float_image_record(encoding, width, 1, &values, true, 3);
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
    let index = build_index(&source, directory.path(), "float-test-sha").unwrap();
    for topic in index["topics"].as_object().unwrap().values() {
        assert_eq!(topic["kind"], "image");
        assert_ne!(topic["unsupported"], true);
        assert!(topic["errors"].as_array().unwrap().is_empty());
        let frames = topic["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 2);
        for (number, frame) in frames.iter().enumerate() {
            let encoding = topic["image_encoding"].as_str().unwrap();
            let mono = encoding == "32FC1";
            let preview = directory
                .path()
                .join("frames")
                .join(frame["image"].as_str().unwrap());
            let pixels = image::open(&preview).unwrap().to_rgb8().into_raw();
            let expected = if number == 0 {
                [63, 127, 191]
            } else {
                [0, 127, 255]
            };
            let reverse = encoding == "32FC3" || encoding == "bgr32f";
            let expected_pixels: Vec<_> = if mono {
                expected.into_iter().flat_map(|v| [v; 3]).collect()
            } else if reverse {
                expected.into_iter().rev().collect()
            } else {
                expected.to_vec()
            };
            assert_eq!(pixels, expected_pixels);
            let output = preview.with_extension("npy");
            let metadata = super::write_training_tensor(&preview, &output, false)
                .unwrap()
                .unwrap();
            assert_eq!(metadata["dtype"], "<f4");
            assert_eq!(metadata["scale"], 1.0);
            assert_eq!(
                metadata["shape"],
                if mono {
                    json!([1, 3])
                } else {
                    json!([1, 1, 3])
                }
            );
            let bytes = fs::read(output).unwrap();
            let header_len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
            let decoded: Vec<_> = bytes[10 + header_len..]
                .chunks_exact(4)
                .map(|part| f32::from_le_bytes(part.try_into().unwrap()))
                .collect();
            let mut values = if number == 0 {
                vec![0.25, 0.5, 0.75]
            } else {
                vec![-2.0, 0.0, 2.0]
            };
            if reverse {
                values.reverse();
            }
            assert_eq!(decoded, values);
        }
    }
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
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../misc/synthetic_robot_arm.mcap");
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

#[test]
fn streaming_translations_preserve_opaque_payloads_schemas_clocks_and_empty_topics() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("mixed.mcap");
    let destination = directory.path().join("translated.mcap");
    let mut writer = mcap::Writer::new(fs::File::create(&source).unwrap()).unwrap();
    let schema = writer
        .add_schema("Motor", "jsonschema", b"{\"type\":\"object\"}")
        .unwrap();
    let motor = writer
        .add_channel(schema, "/motor", "json", &BTreeMap::new())
        .unwrap();
    let video_schema = writer
        .add_schema("OpaqueVideo", "custom", b"opaque-schema")
        .unwrap();
    let video = writer
        .add_channel(video_schema, "/video", "custom", &BTreeMap::new())
        .unwrap();
    let empty = writer
        .add_channel(0, "/empty", "custom", &BTreeMap::new())
        .unwrap();
    let payload = vec![42; 1024 * 1024];
    for (channel, data) in [
        (video, payload.as_slice()),
        (motor, b"{\"value\":3}".as_slice()),
    ] {
        writer
            .write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id: channel,
                    sequence: 7,
                    log_time: 99,
                    publish_time: 98,
                },
                data,
            )
            .unwrap();
    }
    writer.finish().unwrap();
    drop(writer);
    let result=process_recording(&source,&destination,&json!({"selected_channels":[motor.to_string(),video.to_string(),empty.to_string()],"translations":{motor.to_string():"value_mm <- source.value * 1000"},"preserve_untranslated":true,"sync":{"enabled":false}})).unwrap();
    assert_eq!(result["output_messages"], 2);
    let bytes = fs::read(&destination).unwrap();
    let messages = mcap::MessageStream::new(&bytes)
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    let raw = messages
        .iter()
        .find(|message| message.channel.topic == "/video")
        .unwrap();
    assert_eq!(raw.data.as_ref(), payload);
    assert_eq!(raw.channel.message_encoding, "custom");
    let schema = raw.channel.schema.as_ref().unwrap();
    assert_eq!(schema.name, "OpaqueVideo");
    assert_eq!(schema.encoding, "custom");
    assert_eq!(schema.data.as_ref(), b"opaque-schema");
    assert_eq!((raw.sequence, raw.log_time, raw.publish_time), (7, 99, 98));
    let translated = messages
        .iter()
        .find(|message| message.channel.topic == "/motor")
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&translated.data).unwrap(),
        json!({"value_mm":3000})
    );
    assert_eq!(
        (
            translated.sequence,
            translated.log_time,
            translated.publish_time
        ),
        (7, 99, 98)
    );
    let summary = mcap::Summary::read(&bytes).unwrap().unwrap();
    assert!(summary
        .channels
        .values()
        .any(|channel| channel.topic == "/empty"));
    let lookup = super::translation_channels(&destination).unwrap();
    assert_eq!(lookup.as_array().unwrap().len(), 3);
}

#[test]
fn unsynchronized_processing_streams_source_order_and_publishes_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source-order.mcap");
    let output = directory.path().join("output.mcap");
    let mut writer = mcap::Writer::new(File::create(&source).unwrap()).unwrap();
    let channel = writer
        .add_channel(0, "/records", "json", &BTreeMap::new())
        .unwrap();
    for (sequence, time) in [30, 10, 20].into_iter().enumerate() {
        writer
            .write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id: channel,
                    sequence: sequence as u32,
                    log_time: time,
                    publish_time: time + 1,
                },
                &serde_json::to_vec(&json!({"value":time})).unwrap(),
            )
            .unwrap();
    }
    writer.finish().unwrap();
    drop(writer);
    let request = json!({"translations":{channel.to_string():"scaled <- source.value * 2"}});
    let stats = process_recording(&source, &output, &request).unwrap();
    assert_eq!(stats["input_messages"], 3);
    assert_eq!(stats["output_encoding"], "json");
    let mut seen = Vec::new();
    stream_messages(&output, |message| {
        let record: Value = serde_json::from_slice(&message.data)?;
        assert_eq!(record["scaled"], message.log_time * 2);
        assert_eq!(message.publish_time, message.log_time + 1);
        assert_eq!(message.sequence as usize, seen.len());
        seen.push(message.log_time);
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, vec![30, 10, 20]);
    // Preview navigation remains chronological even when physical records are not.
    let index = build_index(&output, directory.path(), "test").unwrap();
    let frames = index["topics"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()["frames"]
        .as_array()
        .unwrap();
    assert_eq!(
        frames
            .iter()
            .map(|f| f["time_ns"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
    let failed = directory.path().join("failed.mcap");
    assert!(process_recording(
        &source,
        &failed,
        &json!({"translations":{channel.to_string():"bad <- source.missing"}})
    )
    .is_err());
    assert!(!failed.exists());
    assert!(process_recording(&source, &output, &request).is_err());
    let empty = directory.path().join("missing-channel.mcap");
    assert!(process_recording(
        &source,
        &empty,
        &json!({"selected_channels":[channel, 999]})
    )
    .is_err());
    assert!(!empty.exists());
}

#[test]
fn byte_image_paths_preserve_pixels_across_layouts_and_padded_rows() {
    use super::images::{decode_image_bytes, payload_bytes};
    for (encoding, first, second) in [
        ("rgb8", vec![12, 34, 56], vec![78, 90, 123]),
        ("bgr8", vec![56, 34, 12], vec![123, 90, 78]),
        ("8UC3", vec![56, 34, 12], vec![123, 90, 78]),
        ("rgba8", vec![12, 34, 56, 11], vec![78, 90, 123, 22]),
        ("bgra8", vec![56, 34, 12, 11], vec![123, 90, 78, 22]),
    ] {
        let step = first.len() + 2;
        let bytes = [first, vec![255, 255], second, vec![255, 255]].concat();
        let record = json!({"width":1,"height":2,"encoding":encoding,"step":step,"data":bytes});
        let image = decode_image_bytes(&record, &payload_bytes(&record["data"]).unwrap()).unwrap();
        assert_eq!(
            image.to_rgb8().into_raw(),
            vec![12, 34, 56, 78, 90, 123],
            "{encoding}"
        );
        let mut short = record.clone();
        short["step"] = json!(1);
        assert!(decode_image(&short).is_err());
    }
    for encoding in ["mono8", "mono_8", "8UC1"] {
        let image = decode_image(&json!({"width":2,"height":2,"encoding":encoding,"step":3,"data":[12,34,255,56,78,255]})).unwrap();
        assert_eq!(
            image.to_rgb8().into_raw(),
            vec![12, 12, 12, 34, 34, 34, 56, 56, 56, 78, 78, 78]
        );
    }
}

#[test]
fn protobuf_index_decoder_preserves_metadata_payloads_and_non_camera_records() {
    use super::images::{decode_image_bytes, payload_bytes};
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../misc/synthetic_robot_arm.mcap");
    let mut decoder = Decoder::default();
    let mut cameras = 0;
    let mut records = 0;
    stream_messages(&fixture, |message| {
        let expected = decoder.decode(message)?;
        let (mut record, data) = decoder.decode_for_index(message)?;
        if is_camera(&expected) {
            cameras += 1;
            let data = data.expect("Protobuf image bytes must remain binary");
            assert_eq!(data, payload_bytes(&expected["data"])?);
            assert_eq!(
                decode_image_bytes(&record, &data)?.to_rgb8(),
                decode_image(&expected)?.to_rgb8()
            );
            record["data"] = expected["data"].clone();
        } else {
            records += 1;
            assert!(data.is_none());
        }
        assert_eq!(record, expected);
        Ok(())
    })
    .unwrap();
    assert!(cameras > 0 && records > 0);
}

#[test]
fn indexing_digest_matches_entire_compressed_and_unchunked_files() {
    use std::io::Write;
    for chunked in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.mcap");
        let mut writer = mcap::WriteOptions::default()
            .use_chunks(chunked)
            .create(File::create(&source).unwrap())
            .unwrap();
        let channel = writer
            .add_channel(0, "/records", "json", &BTreeMap::new())
            .unwrap();
        for sequence in 0..100 {
            writer
                .write_to_known_channel(
                    &mcap::records::MessageHeader {
                        channel_id: channel,
                        sequence,
                        log_time: sequence as u64,
                        publish_time: sequence as u64,
                    },
                    &serde_json::to_vec(&json!({"value":sequence,"padding":"x".repeat(1024)}))
                        .unwrap(),
                )
                .unwrap();
        }
        writer.finish().unwrap();
        drop(writer);
        // File identity must include bytes outside the parsed MCAP as well.
        fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .unwrap()
            .write_all(b"trailing identity bytes")
            .unwrap();
        let expected = crate::storage::file_hash(&source).unwrap();
        let index = build_index_and_hash(&source, &dir.path().join("combined")).unwrap();
        assert_eq!(index["sha256"], expected);
        assert_eq!(
            index,
            build_index(&source, &dir.path().join("separate"), &expected).unwrap()
        );
    }
}

#[test]
fn training_tensor_reuse_checks_source_mode_and_output_identity() {
    let dir = tempfile::tempdir().unwrap();
    let preview = dir.path().join("depth.png");
    let output = dir.path().join("depth.npy");
    let mut record =
        json!({"width":2,"height":1,"step":6,"encoding":"16UC1","data":[1,0,2,0,99,99]});
    super::training_images::preserve_native(&record, &preview, true).unwrap();
    let meta = write_training_tensor(&preview, &output, true).unwrap();
    let stamp = crate::storage::fingerprint(&output).unwrap();
    assert_eq!(
        write_training_tensor(&preview, &output, true).unwrap(),
        meta
    );
    assert_eq!(crate::storage::fingerprint(&output).unwrap(), stamp);
    let original = fs::read(&output).unwrap();
    assert_eq!(&original[original.len() - 4..], &[1, 0, 2, 0]);
    record["data"] = json!([3, 0, 4, 0, 99, 99]);
    super::training_images::preserve_native(&record, &preview, true).unwrap();
    write_training_tensor(&preview, &output, true).unwrap();
    let changed = fs::read(&output).unwrap();
    assert_eq!(&changed[changed.len() - 4..], &[3, 0, 4, 0]);
    fs::write(&output, b"interrupted tensor").unwrap();
    write_training_tensor(&preview, &output, true).unwrap();
    assert_eq!(fs::read(&output).unwrap(), changed);
    let color = write_training_tensor(&preview, &output, false)
        .unwrap()
        .unwrap();
    assert_eq!(color["scale"], 65535.0);
}
