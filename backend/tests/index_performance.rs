//! Repeatable, opt-in preview benchmark. Fixture generation is excluded from timing.
use mcap_workbench::engine::{build_index, build_index_and_hash};
use prost::Message;
use prost_types::{
    field_descriptor_proto::{Label, Type},
    DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
};
use std::{collections::BTreeMap, fs::File, io::BufWriter, time::Instant};

#[derive(Clone, PartialEq, Message)]
struct Camera {
    #[prost(uint32, tag = "1")]
    width: u32,
    #[prost(uint32, tag = "2")]
    height: u32,
    #[prost(string, tag = "3")]
    encoding: String,
    #[prost(uint32, tag = "4")]
    step: u32,
    #[prost(bytes = "vec", tag = "5")]
    data: Vec<u8>,
}

#[test]
#[ignore = "preview throughput benchmark; run with --release --ignored --nocapture"]
fn protobuf_camera_index_throughput() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("camera.mcap");
    let fields = [
        ("width", Type::Uint32),
        ("height", Type::Uint32),
        ("encoding", Type::String),
        ("step", Type::Uint32),
        ("data", Type::Bytes),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, kind))| FieldDescriptorProto {
        name: Some(name.into()),
        number: Some(i as i32 + 1),
        label: Some(Label::Optional as i32),
        r#type: Some(kind as i32),
        ..Default::default()
    })
    .collect();
    let schema = FileDescriptorSet {
        file: vec![FileDescriptorProto {
            name: Some("camera.proto".into()),
            syntax: Some("proto3".into()),
            message_type: vec![DescriptorProto {
                name: Some("Camera".into()),
                field: fields,
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
    .encode_to_vec();
    let mut writer = mcap::Writer::new(BufWriter::new(File::create(&source).unwrap())).unwrap();
    let schema = writer.add_schema("Camera", "protobuf", &schema).unwrap();
    let channel = writer
        .add_channel(schema, "/camera/rgb", "protobuf", &BTreeMap::new())
        .unwrap();
    let mut state = 42u32;
    let data = (0..640 * 480 * 3)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 24) as u8
        })
        .collect();
    let camera = Camera {
        width: 640,
        height: 480,
        encoding: "bgr8".into(),
        step: 640 * 3,
        data,
    }
    .encode_to_vec();
    let frames = 60;
    for sequence in 0..frames {
        writer
            .write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id: channel,
                    sequence,
                    log_time: sequence as u64,
                    publish_time: sequence as u64,
                },
                &camera,
            )
            .unwrap();
    }
    writer.finish().unwrap();
    drop(writer);
    let started = Instant::now();
    let index = build_index(&source, directory.path(), "benchmark").unwrap();
    let elapsed = started.elapsed();
    let topic = &index["topics"][channel.to_string()];
    assert_eq!(topic["frames"].as_array().unwrap().len(), frames as usize);
    assert_eq!(topic["errors"], serde_json::json!([]));
    eprintln!(
        "Indexed {frames} Protobuf BGR8 frames (640x480) in {:.3}s: {:.1} frames/s",
        elapsed.as_secs_f64(),
        frames as f64 / elapsed.as_secs_f64()
    );
    let separate_started = Instant::now();
    let hash = mcap_workbench::storage::file_hash(&source).unwrap();
    let separate = build_index(&source, &directory.path().join("separate"), &hash).unwrap();
    let separate_elapsed = separate_started.elapsed();
    let combined_started = Instant::now();
    let combined = build_index_and_hash(&source, &directory.path().join("combined")).unwrap();
    let combined_elapsed = combined_started.elapsed();
    assert_eq!(combined, separate);
    eprintln!(
        "Hash + index: separate reads {:.3}s, combined read {:.3}s",
        separate_elapsed.as_secs_f64(),
        combined_elapsed.as_secs_f64()
    );
}
