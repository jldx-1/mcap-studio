use base64::{engine::general_purpose::STANDARD, Engine};
use mcap_workbench::{engine, Store};
use openh264::{
    encoder::{Encoder, EncoderConfig, FrameType},
    formats::{RgbSliceU8, YUVBuffer},
    OpenH264API, Timestamp,
};
use prost::Message;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::Path,
};

#[derive(Clone, PartialEq, Message)]
struct CompressedVideo {
    #[prost(bytes = "vec", tag = "1")]
    data: Vec<u8>,
    #[prost(string, tag = "2")]
    format: String,
}
fn schema() -> Vec<u8> {
    use prost_types::{
        field_descriptor_proto::{Label, Type},
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
    };
    FileDescriptorSet {
        file: vec![FileDescriptorProto {
            name: Some("video.proto".into()),
            package: Some("foxglove".into()),
            syntax: Some("proto3".into()),
            message_type: vec![DescriptorProto {
                name: Some("CompressedVideo".into()),
                field: vec![
                    FieldDescriptorProto {
                        name: Some("data".into()),
                        number: Some(1),
                        label: Some(Label::Optional as i32),
                        r#type: Some(Type::Bytes as i32),
                        ..Default::default()
                    },
                    FieldDescriptorProto {
                        name: Some("format".into()),
                        number: Some(2),
                        label: Some(Label::Optional as i32),
                        r#type: Some(Type::String as i32),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
    .encode_to_vec()
}
fn packets(blue: bool) -> Vec<Vec<u8>> {
    let mut encoder = Encoder::with_api_config(
        OpenH264API::from_source(),
        EncoderConfig::new().skip_frames(false),
    )
    .unwrap();
    (0..12)
        .map(|index| {
            let pixel = if blue {
                [10, 40 + index * 4, 200]
            } else {
                [200, 40 + index * 4, 10]
            };
            let rgb: Vec<_> = (0..32 * 32).flat_map(|_| pixel).collect();
            let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(&rgb, (32, 32)));
            let encoded = encoder
                .encode_at(&yuv, Timestamp::from_millis(index as u64 * 100))
                .unwrap();
            if index == 0 {
                assert_eq!(encoded.frame_type(), FrameType::IDR);
            } else {
                assert_eq!(encoded.frame_type(), FrameType::P);
            }
            encoded.to_vec()
        })
        .collect()
}
fn message(writer: &mut mcap::Writer<File>, channel: u16, sequence: u32, time: u64, bytes: &[u8]) {
    writer
        .write_to_known_channel(
            &mcap::records::MessageHeader {
                channel_id: channel,
                sequence,
                log_time: time,
                publish_time: time,
            },
            bytes,
        )
        .unwrap();
}
fn fixture(path: &Path) {
    let mut red = packets(false);
    let raw_keyframe = red[0].clone();
    let blue = packets(true);
    let mut writer = mcap::Writer::new(File::create(path).unwrap()).unwrap();
    let schema = writer
        .add_schema("foxglove.CompressedVideo", "protobuf", &schema())
        .unwrap();
    let h264 = writer
        .add_channel(schema, "/camera/h264", "protobuf", &BTreeMap::new())
        .unwrap();
    let jpeg = writer
        .add_channel(0, "/camera/jpeg", "json", &BTreeMap::new())
        .unwrap();
    let other = writer
        .add_channel(0, "/camera/other-h264", "json", &BTreeMap::new())
        .unwrap();
    let direct = writer
        .add_channel(0, "/camera/direct-jpeg", "video/jpeg", &BTreeMap::new())
        .unwrap();
    let direct_h264 = writer
        .add_channel(0, "/camera/direct-h264", "video/h264", &BTreeMap::new())
        .unwrap();
    let sensor = writer
        .add_channel(0, "/motor", "json", &BTreeMap::new())
        .unwrap();
    // Parameter-set messages must not produce phantom frames or shift timestamps.
    let headers: Vec<u8> = openh264::nal_units(&red[0])
        .filter(|nal| matches!(nal[3] & 31, 7 | 8))
        .flatten()
        .copied()
        .collect();
    message(
        &mut writer,
        h264,
        0,
        1_000_000_000,
        &CompressedVideo {
            data: headers,
            format: "H264".into(),
        }
        .encode_to_vec(),
    );
    red[0] = openh264::nal_units(&red[0])
        .filter(|nal| !matches!(nal[3] & 31, 7 | 8))
        .flatten()
        .copied()
        .collect();
    for index in 0..12 {
        let time = 1_000_000_001 + index as u64 * 100_000_000;
        message(
            &mut writer,
            direct_h264,
            index as u32,
            time,
            if index == 0 {
                &raw_keyframe
            } else {
                &red[index]
            },
        );
        message(
            &mut writer,
            h264,
            index as u32 + 1,
            time,
            &CompressedVideo {
                data: red[index].clone(),
                format: "h264".into(),
            }
            .encode_to_vec(),
        );
        message(
            &mut writer,
            other,
            index as u32,
            time,
            &serde_json::to_vec(
                &json!({"codec":"video/h264","data":STANDARD.encode(&blue[index])}),
            )
            .unwrap(),
        );
        let rgb: Vec<u8> = (0..32 * 32).flat_map(|_| [15, 180, 30]).collect();
        let mut image = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut image, 95)
            .encode(&rgb, 32, 32, image::ExtendedColorType::Rgb8)
            .unwrap();
        message(&mut writer, direct, index as u32, time, &image);
        message(
            &mut writer,
            jpeg,
            index as u32,
            time,
            &serde_json::to_vec(
                &json!({"format":"video/jpeg","data":{"$bytes_base64":STANDARD.encode(&image)}}),
            )
            .unwrap(),
        );
        message(
            &mut writer,
            sensor,
            index as u32,
            time,
            &serde_json::to_vec(&json!({"position":index})).unwrap(),
        );
    }
    writer.finish().unwrap();
}
#[test]
fn h264_delta_frames_and_jpeg_video_are_indexed_with_independent_decoders() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("video.mcap");
    fixture(&source);
    let cache = dir.path().join("cache");
    let index = engine::build_index(&source, &cache, "fixture").unwrap();
    for channel in ["1", "2", "3", "4", "5"] {
        let topic = &index["topics"][channel];
        assert_eq!(topic["kind"], "image");
        assert_eq!(topic["errors"], json!([]));
        let frames = topic["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 12);
        for (number, frame) in frames.iter().enumerate() {
            assert_eq!(
                frame["time_ns"],
                1_000_000_001 + number as u64 * 100_000_000
            );
            assert_eq!(frame["width"], 32);
            assert_eq!(frame["height"], 32);
            let preview = cache.join("frames").join(frame["image"].as_str().unwrap());
            let image = image::open(&preview).unwrap().to_rgb8();
            let pixel = image.get_pixel(16, 16).0;
            let expected = match channel {
                "1" | "5" => [200, 40 + number as u8 * 4, 10],
                "2" | "4" => [15, 180, 30],
                _ => [10, 40 + number as u8 * 4, 200],
            };
            for (actual, expected) in pixel.into_iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) < 15,
                    "channel {channel} frame {number}: {pixel:?} != {expected:?}"
                );
            }
            assert!(!preview.with_extension("native.bin").exists());
            assert_eq!(
                engine::write_training_tensor(&preview, &dir.path().join("tensor.npy"), false)
                    .unwrap(),
                None
            );
        }
    }
    engine::ensure_native_training_images(
        &source,
        &cache,
        &index,
        &["1".into(), "2".into(), "3".into()],
    )
    .unwrap();
    assert!(cache.join("frames/native-training-index.json").is_file());
}
#[test]
fn old_h264_failures_rebuild_when_recordings_are_reopened() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let project = store
        .create_project(&json!({"name":"Compressed cameras"}))
        .unwrap();
    let source = dir.path().join("video.mcap");
    fixture(&source);
    let id = project["uuid"].as_str().unwrap();
    let recording = store.import_recording(id, "video.mcap", &source).unwrap();
    let uuid = recording["uuid"].as_str().unwrap();
    let index_path =
        Path::new(project["path"].as_str().unwrap()).join(format!("mcap/{uuid}/frames/index.json"));
    let mut index: Value = serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
    index["image_decoder_version"] = json!(2);
    index["topics"]["1"]["frames"] = json!([]);
    index["topics"]["1"]["errors"] =
        json!(["At 100: Compressed video (h264) is not supported by the native image decoder"]);
    fs::write(&index_path, serde_json::to_vec(&index).unwrap()).unwrap();
    let reopened = store.recording(id, uuid).unwrap();
    assert_eq!(
        reopened["index"]["topics"]["1"]["frames"]
            .as_array()
            .unwrap()
            .len(),
        12
    );
    assert_eq!(reopened["index"]["topics"]["1"]["errors"], json!([]));
}
#[test]
fn malformed_packets_report_actionable_errors_without_losing_other_topics() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("bad-video.mcap");
    let mut writer = mcap::Writer::new(File::create(&source).unwrap()).unwrap();
    let camera = writer
        .add_channel(0, "/video", "json", &BTreeMap::new())
        .unwrap();
    let sensor = writer
        .add_channel(0, "/sensor", "json", &BTreeMap::new())
        .unwrap();
    message(
        &mut writer,
        camera,
        0,
        100,
        &serde_json::to_vec(&json!({"format":"h264","data":[1,2,3]})).unwrap(),
    );
    message(&mut writer, sensor, 0, 100, br#"{"value":1}"#);
    writer.finish().unwrap();
    drop(writer);
    let index = engine::build_index(&source, &dir.path().join("cache"), "fixture").unwrap();
    assert!(index["topics"]["1"]["errors"][0]
        .as_str()
        .unwrap()
        .contains("Annex B"));
    assert_eq!(index["topics"]["2"]["frames"].as_array().unwrap().len(), 1);
}
#[test]
fn invalid_video_layouts_are_reported_without_misaligning_frames() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("invalid.mcap");
    let mut writer = mcap::Writer::new(File::create(&source).unwrap()).unwrap();
    let encoded = packets(false);
    let cases = [
        ("b-frames", "h264", vec![0, 0, 1, 0x41, 0xa0], "B-frames"),
        (
            "multiple-frames",
            "h264",
            [encoded[0].as_slice(), encoded[1].as_slice()].concat(),
            "exactly one",
        ),
        ("missing-keyframe", "h264", encoded[1].clone(), "SPS/PPS"),
        ("bad-jpeg", "video/jpeg", vec![1, 2, 3], "JPEG/MJPEG"),
    ];
    for (topic, format, data, _) in &cases {
        let channel = writer
            .add_channel(0, topic, "json", &BTreeMap::new())
            .unwrap();
        message(
            &mut writer,
            channel,
            0,
            100,
            &serde_json::to_vec(&json!({"format":format,"data":data})).unwrap(),
        );
    }
    writer.finish().unwrap();
    drop(writer);
    let index = engine::build_index(&source, &dir.path().join("cache"), "fixture").unwrap();
    for (number, (_, _, _, expected)) in cases.iter().enumerate() {
        let topic = &index["topics"][(number + 1).to_string()];
        assert!(topic["frames"].as_array().unwrap().is_empty());
        assert!(
            topic["errors"][0].as_str().unwrap().contains(expected),
            "{}",
            topic["errors"]
        );
    }
}

#[test]
#[ignore = "generates an MCAP fixture for browser tests"]
fn write_browser_fixture() {
    fixture(Path::new(
        &std::env::var("MCAP_VIDEO_FIXTURE").expect("Set MCAP_VIDEO_FIXTURE"),
    ));
}
