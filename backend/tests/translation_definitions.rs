use base64::{engine::general_purpose::STANDARD, Engine};
use mcap_workbench::Store;
use serde_json::json;
#[test]
fn protobuf_definitions_decode_binary_and_save_csv_mappings() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let (actor, _) = store
        .accounts
        .setup(
            &json!({"username":"admin","name":"Admin","password":"definition-test-password-123"}),
        )
        .unwrap();
    let project = store
        .create_project(&json!({"name":"Definition ETL"}))
        .unwrap();
    let id = project["uuid"].as_str().unwrap();
    let proto =
        b"syntax=\"proto3\"; package test; message Sensor { double speed=1; int64 ticks=2; }";
    let loaded = store
        .upload_translation_definitions(
            id,
            &json!({"files":[{"name":"sensor.proto","data_base64":STANDARD.encode(proto)}]}),
            &actor,
        )
        .unwrap();
    let definition = &loaded["definitions"][0];
    assert_eq!(definition["message"], "test.Sensor");
    let mut bytes = vec![9];
    bytes.extend_from_slice(&1.5f64.to_le_bytes());
    bytes.extend_from_slice(&[16, 42]);
    let request = json!({"definition_id":definition["id"],"message":"test.Sensor","sample_base64":STANDARD.encode(bytes),"script":"speed_mm <- source.speed * 1000\nticks <- source.ticks","name":"Sensor CSV"});
    let preview = store.preview_definition_translation(id, &request).unwrap();
    assert_eq!(preview["row"]["/speed_mm"], 1500.0);
    assert_eq!(preview["row"]["/ticks"], 42);
    let saved = store
        .save_definition_translation(id, &request, &actor)
        .unwrap();
    assert_eq!(saved["output_format"], "csv");
    assert!(saved.get("topic").is_none());
    assert_eq!(
        store.translation_definitions(id).unwrap()["definitions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let invalid = json!({"definition_id":definition["id"],"message":"test.Sensor","sample":{"unknown":3},"script":"* -> *"});
    assert!(store.preview_definition_translation(id, &invalid).is_err());
    assert!(store
        .upload_translation_definitions(
            id,
            &json!({"files":[{"name":"../escape.proto","data_base64":STANDARD.encode(proto)}]}),
            &actor
        )
        .is_err());
}
#[test]
fn descriptor_sets_work_without_a_proto_compiler() {
    use prost::Message;
    use prost_types::{DescriptorProto, FileDescriptorProto, FileDescriptorSet};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let (actor, _) = store
        .accounts
        .setup(
            &json!({"username":"admin","name":"Admin","password":"definition-test-password-123"}),
        )
        .unwrap();
    let project = store
        .create_project(&json!({"name":"Compiled definitions"}))
        .unwrap();
    let id = project["uuid"].as_str().unwrap();
    let bytes = FileDescriptorSet {
        file: vec![FileDescriptorProto {
            name: Some("empty.proto".into()),
            syntax: Some("proto3".into()),
            message_type: vec![DescriptorProto {
                name: Some("Empty".into()),
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
    .encode_to_vec();
    let result = store
        .upload_translation_definitions(
            id,
            &json!({"files":[{"name":"types.desc","data_base64":STANDARD.encode(bytes)}]}),
            &actor,
        )
        .unwrap();
    assert_eq!(result["definitions"][0]["message"], "Empty");
}
