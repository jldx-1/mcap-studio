//! Timestamped decoded frame and record cache construction.
use anyhow::{Context, Result};
use image::{
    codecs::png::{CompressionType, FilterType, PngEncoder},
    ImageEncoder,
};
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

use super::{
    images::payload_bytes,
    is_camera,
    reader::stream_messages_with_hash,
    video::{compressed_format, CameraDecoder},
    Decoder,
};

const IMAGE_DECODER_VERSION: u32 = 3;

/// Retry cached camera-format failures once when decoder support improves.
pub fn image_cache_needs_upgrade(index: &Value) -> bool {
    index["image_decoder_version"] != IMAGE_DECODER_VERSION
        && index["topics"].as_object().is_some_and(|topics| {
            topics.values().any(|topic| {
                topic["errors"].as_array().is_some_and(|errors| {
                    errors.iter().any(|error| {
                        error.as_str().is_some_and(|text| {
                            text.contains("Unsupported raw image encoding")
                                || text
                                    .to_ascii_lowercase()
                                    .contains("compressed video (h264)")
                                || text
                                    .split_once("Unsupported message encoding ")
                                    .is_some_and(|(_, suffix)| {
                                        let encoding = suffix
                                            .split_whitespace()
                                            .next()
                                            .unwrap_or("")
                                            .trim_end_matches(';');
                                        compressed_format(&json!({"format":encoding})).is_some()
                                    })
                        })
                    })
                })
            })
        })
}

/// Build a legacy-compatible timestamped image/record cache in `cache_dir/frames`.
pub fn build_index(recording: &Path, cache_dir: &Path, sha256: &str) -> Result<Value> {
    build_index_inner(recording, cache_dir, Some(sha256))
}

/// Build previews and compute the MCAP identity in one sequential source read.
pub fn build_index_and_hash(recording: &Path, cache_dir: &Path) -> Result<Value> {
    build_index_inner(recording, cache_dir, None)
}

fn build_index_inner(recording: &Path, cache_dir: &Path, sha256: Option<&str>) -> Result<Value> {
    fs::create_dir_all(cache_dir)?;
    let temporary = tempfile::Builder::new()
        .prefix(".frames-")
        .tempdir_in(cache_dir)?;
    let mut decoder = Decoder::default();
    let mut cameras = CameraDecoder::default();
    let mut topics = Map::new();
    let mut records: HashMap<u16, (BufWriter<File>, u64)> = HashMap::new();
    let (declared, calculated) = stream_messages_with_hash(
        recording,
        sha256.is_none(),
        |message| {
            let channel = &message.channel;
            let key = channel.id.to_string();
            if !topics.contains_key(&key) {
                topics.insert(key.clone(), topic_metadata(channel));
                fs::create_dir(temporary.path().join(format!("channel_{}", channel.id)))?;
            }
            let topic = topics.get_mut(&key).unwrap();
            topic["message_count"] = json!(topic["message_count"].as_u64().unwrap_or(0) + 1);
            if topic["unsupported"] == true {
                return Ok(());
            }
            let result = (|| -> Result<()> {
                let (record, payload) = decoder.decode_for_index(message)?;
                if is_camera(&record) {
                    let bytes = if let Some(payload) = payload {
                        payload
                    } else {
                        payload_bytes(&record["data"])?
                    };
                    topic["kind"] = json!("image");
                    topic["image_encoding"] = compressed_format(&record)
                        .map(|format| json!(format))
                        .unwrap_or_else(|| {
                            record
                                .get("pixel_format")
                                .or(record.get("encoding"))
                                .cloned()
                                .unwrap_or(Value::Null)
                        });
                    let Some(image) = cameras.decode(channel.id, &record, &bytes)? else {
                        return Ok(());
                    };
                    let relative = format!(
                        "channel_{}/{:08}.png",
                        channel.id,
                        topic["frames"].as_array().unwrap().len()
                    );
                    let preview = temporary.path().join(&relative);
                    let mut output = BufWriter::new(File::create(&preview)?);
                    PngEncoder::new_with_quality(
                        &mut output,
                        CompressionType::Fast,
                        FilterType::Adaptive,
                    )
                    .write_image(
                        image.as_bytes(),
                        image.width(),
                        image.height(),
                        image.color().into(),
                    )?;
                    output.flush()?;
                    super::training_images::preserve_native_bytes(
                        &record,
                        &bytes,
                        &preview,
                        channel.topic.to_ascii_lowercase().contains("depth"),
                    )?;
                    topic["frames"].as_array_mut().unwrap().push(json!({"time_ns": message.log_time, "image": relative, "width": image.width(), "height": image.height()}));
                } else {
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        records.entry(channel.id)
                    {
                        let file = File::create(
                            temporary
                                .path()
                                .join(format!("channel_{}/records.jsonl", channel.id)),
                        )?;
                        entry.insert((BufWriter::new(file), 0));
                    }
                    let (writer, offset) = records.get_mut(&channel.id).unwrap();
                    let mut bytes = serde_json::to_vec(&record)?;
                    bytes.push(b'\n');
                    writer.write_all(&bytes)?;
                    topic["frames"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"time_ns": message.log_time, "offset": *offset}));
                    *offset += bytes.len() as u64;
                }
                Ok(())
            })();
            if let Err(error) = result {
                let detail = format!("{error:#}");
                if detail.contains("Unsupported") || detail.contains("not supported") {
                    topic["unsupported"] = json!(true);
                }
                let errors = topic["errors"].as_array_mut().unwrap();
                if errors.len() < 5 {
                    errors.push(json!(format!("At {}: {detail}", message.log_time)));
                }
            }
            Ok(())
        },
    )?;
    // Include declared channels without messages.
    for channel in declared.values() {
        topics
            .entry(channel.id.to_string())
            .or_insert_with(|| topic_metadata(channel));
    }
    for (writer, _) in records.values_mut() {
        writer.flush()?;
    }
    for topic in topics.values_mut() {
        topic["frames"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|frame| frame["time_ns"].as_u64().unwrap_or(0));
    }
    let sha256 = sha256
        .map(str::to_owned)
        .or(calculated)
        .context("Missing recording digest")?;
    let index = json!({"format_version": 1, "image_decoder_version": IMAGE_DECODER_VERSION, "sha256": sha256, "topics": topics});
    let mut index_file = BufWriter::new(File::create(temporary.path().join("index.json"))?);
    serde_json::to_writer(&mut index_file, &index)?;
    index_file.flush()?;
    drop(index_file);
    drop(records);
    let destination = cache_dir.join("frames");
    // Existing caches are never modified until decoding has fully completed.
    let backup = cache_dir.join(format!(".frames-backup-{}", uuid::Uuid::new_v4()));
    if destination.exists() {
        fs::rename(&destination, &backup)?;
    }
    if let Err(error) = fs::rename(temporary.path(), &destination) {
        if backup.exists() {
            let _ = fs::rename(&backup, &destination);
        }
        return Err(error.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    Ok(index)
}

pub(super) fn topic_metadata(channel: &mcap::Channel<'_>) -> Value {
    let schema = channel
        .schema
        .as_ref()
        .map(|schema| schema.name.as_str())
        .unwrap_or("");
    json!({"channel_id": channel.id, "topic": channel.topic, "schema": schema, "schema_name": schema,
        "encoding": channel.message_encoding, "kind": "record", "technique": channel.metadata.get("labeling_technique").map(String::as_str).unwrap_or("undefined"),
        "frames": [], "errors": [], "message_count": 0})
}
