use mcap_workbench::engine::recording_metadata;
use std::{collections::BTreeMap, fs::File};

#[test]
fn sensor_only_recordings_have_metrics_without_previews() {
    for summary in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sensors.mcap");
        let mut writer = mcap::WriteOptions::new()
            .emit_summary_records(summary)
            .create(File::create(&path).unwrap())
            .unwrap();
        let channel = writer
            .add_channel(0, "/motor", "json", &BTreeMap::new())
            .unwrap();
        for (sequence, offset) in [0, 2_500_000_000].into_iter().enumerate() {
            writer
                .write_to_known_channel(
                    &mcap::records::MessageHeader {
                        channel_id: channel,
                        sequence: sequence as u32,
                        log_time: 1_800_000_000_000_000_001 + offset,
                        publish_time: 1_800_000_000_000_000_001 + offset,
                    },
                    br#"{"position":1}"#,
                )
                .unwrap();
        }
        writer.finish().unwrap();
        let metrics = recording_metadata(&path).unwrap();
        assert_eq!(metrics["topic_count"], 1);
        assert_eq!(metrics["duration_seconds"], 2.5);
        assert_eq!(
            metrics["file_size_bytes"],
            std::fs::metadata(path).unwrap().len()
        );
        assert!(!dir.path().join("frames").exists());
    }
}

#[test]
fn empty_recordings_have_zero_duration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.mcap");
    let mut writer = mcap::Writer::new(File::create(&path).unwrap()).unwrap();
    writer.finish().unwrap();
    assert_eq!(recording_metadata(&path).unwrap()["duration_seconds"], 0.0);
}
