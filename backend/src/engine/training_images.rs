//! Preserve original high precision camera values for training separately from
//! RGB8 display previews. Tensor files use the standard NumPy .npy format.
use super::images::{image_encoding, payload_bytes, RawFormat};
use super::{decode_image, is_camera, stream_messages, Decoder};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, io::Write, path::Path};

pub(super) fn preserve_native(record: &Value, preview: &Path, depth: bool) -> Result<bool> {
    let data = payload_bytes(&record["data"])?;
    preserve_native_bytes(record, &data, preview, depth)
}

pub(super) fn preserve_native_bytes(
    record: &Value,
    data: &[u8],
    preview: &Path,
    depth: bool,
) -> Result<bool> {
    let encoding = image_encoding(record);
    let png16 = data.starts_with(b"\x89PNG") && data.get(24) == Some(&16);
    if !depth && !png16 && !RawFormat::parse(&encoding).is_ok_and(|format| format.sample_bytes > 1)
    {
        return Ok(false);
    }
    // Copy metadata only: cloning the full record first duplicates a potentially
    // multi-megabyte image payload for every high precision frame.
    let metadata = Value::Object(
        record
            .as_object()
            .context("Image metadata must be an object")?
            .iter()
            .filter(|(key, _)| key.as_str() != "data")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    );
    fs::write(preview.with_extension("native.bin"), data)?;
    fs::write(
        preview.with_extension("native.json"),
        serde_json::to_vec(&metadata)?,
    )?;
    Ok(true)
}
pub fn ensure_native_training_images(
    recording: &Path,
    cache: &Path,
    index: &Value,
    channels: &[String],
) -> Result<()> {
    let marker = cache.join("frames").join("native-training-index.json");
    if let Ok(bytes) = fs::read(&marker) {
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            if value["sha256"] == index["sha256"]
                && channels.iter().all(|ch| {
                    value["channels"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v == ch))
                })
            {
                return Ok(());
            }
        }
    }
    let missing: Vec<_> = channels
        .iter()
        .filter(|ch| {
            index["topics"][*ch]["frames"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|f| {
                    f["image"].as_str().is_some_and(|p| {
                        !cache
                            .join("frames")
                            .join(p)
                            .with_extension("native.json")
                            .is_file()
                    })
                })
        })
        .collect();
    if missing.is_empty() {
        fs::write(
            marker,
            serde_json::to_vec(&json!({"sha256":index["sha256"],"channels":channels}))?,
        )?;
        return Ok(());
    }
    let mut decoder = Decoder::default();
    let mut ordinals: HashMap<u16, usize> = HashMap::new();
    stream_messages(recording, |message| {
        let channel = message.channel.id.to_string();
        if !missing.iter().any(|ch| **ch == channel) {
            return Ok(());
        }
        let record = decoder.decode(message)?;
        if is_camera(&record) && decode_image(&record).is_ok() {
            let ordinal = ordinals.entry(message.channel.id).or_default();
            let preview = cache.join("frames").join(format!(
                "channel_{}/{:08}.png",
                message.channel.id, *ordinal
            ));
            if !preview.with_extension("native.json").is_file() {
                preserve_native(
                    &record,
                    &preview,
                    message.channel.topic.to_ascii_lowercase().contains("depth"),
                )?;
            }
            *ordinal += 1;
        }
        Ok(())
    })?;
    fs::write(
        marker,
        serde_json::to_vec(&json!({"sha256":index["sha256"],"channels":channels}))?,
    )?;
    Ok(())
}
pub fn write_training_tensor(preview: &Path, output: &Path, depth: bool) -> Result<Option<Value>> {
    let metadata_path = preview.with_extension("native.json");
    if !metadata_path.is_file() {
        return Ok(None);
    }
    let source_path = preview.with_extension("native.bin");
    let provenance = json!({
        "metadata": crate::storage::fingerprint(&metadata_path)?,
        "source": crate::storage::fingerprint(&source_path)?,
        "depth": depth,
        "builder_version": 2,
    });
    let reuse_path = output.with_extension("npy.json");
    if let Ok(saved) = crate::storage::read_json(&reuse_path) {
        if saved["source"] == provenance
            && crate::storage::fingerprint(output).is_ok_and(|stamp| saved["output"] == stamp)
            && saved["tensor"].is_object()
        {
            return Ok(Some(saved["tensor"].clone()));
        }
    }
    let metadata: Value = serde_json::from_slice(&fs::read(&metadata_path)?)?;
    let source = fs::read(&source_path)?;
    let encoding = image_encoding(&metadata);
    let (mut width, mut height) = (
        metadata["width"].as_u64().unwrap_or(0) as usize,
        metadata["height"].as_u64().unwrap_or(0) as usize,
    );
    let (descriptor, channels, bytes) = if source.starts_with(b"\x89PNG") {
        let image = image::load_from_memory(&source)?;
        width = image.width() as usize;
        height = image.height() as usize;
        if depth && source.get(24) == Some(&8) {
            ("|u1", 1, image.to_luma8().into_raw())
        } else if depth {
            (
                "<u2",
                1,
                image
                    .to_luma16()
                    .into_raw()
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
        } else {
            (
                "<u2",
                3,
                image
                    .to_rgb16()
                    .into_raw()
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
        }
    } else {
        let layout = RawFormat::parse(&encoding)?;
        let (descriptor, size, channels) =
            (layout.descriptor(), layout.sample_bytes, layout.channels);
        let stride = metadata["step"]
            .as_u64()
            .unwrap_or((width * channels * size) as u64) as usize;
        ensure!(
            width > 0
                && height > 0
                && width.checked_mul(height).is_some_and(|n| n <= 100_000_000)
                && stride >= width * channels * size
                && height
                    .checked_mul(stride)
                    .is_some_and(|n| n <= source.len()),
            "Native image dimensions do not match bytes"
        );
        let big = metadata["is_bigendian"]
            .as_bool()
            .unwrap_or(metadata["is_bigendian"] == 1);
        let reverse = layout.reverse;
        let row_bytes = width * channels * size;
        let mut bytes = Vec::with_capacity(width * height * channels * size);
        if !big && !reverse {
            // Preserve native little-endian rows directly, excluding padding.
            for row in source.chunks_exact(stride).take(height) {
                bytes.extend_from_slice(&row[..row_bytes]);
            }
        } else {
            for y in 0..height {
                for x in 0..width {
                    for c in 0..channels {
                        let channel = if reverse && c < 3 { 2 - c } else { c };
                        let offset = y * stride + (x * channels + channel) * size;
                        let part = &source[offset..offset + size];
                        if big {
                            bytes.extend(part.iter().rev());
                        } else {
                            bytes.extend_from_slice(part);
                        }
                    }
                }
            }
        }
        (descriptor, channels, bytes)
    };
    let shape = if channels == 1 {
        format!("({height}, {width})")
    } else {
        format!("({height}, {width}, {channels})")
    };
    let mut header =
        format!("{{'descr': '{descriptor}', 'fortran_order': False, 'shape': {shape}, }}");
    while (10 + header.len() + 1) % 64 != 0 {
        header.push(' ');
    }
    header.push('\n');
    ensure!(
        header.len() <= u16::MAX as usize,
        "Tensor header is too large"
    );
    {
        // Rebuild when provenance or the output fingerprint changed; this also
        // upgrades old tensors that have no reuse metadata yet.
        fs::create_dir_all(output.parent().context("Missing tensor directory")?)?;
        let mut file = tempfile::NamedTempFile::new_in(output.parent().unwrap())?;
        file.write_all(b"\x93NUMPY\x01\x00")?;
        file.write_all(&(header.len() as u16).to_le_bytes())?;
        file.write_all(header.as_bytes())?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(output).map_err(|e| e.error)?;
    }
    let intensity_scale = RawFormat::parse(&encoding).map_or(65535.0, |layout| {
        if layout.sample_bytes == 2 {
            layout.max_value as f64
        } else {
            65535.0
        }
    });
    let tensor = json!({"dtype":descriptor,"shape":if channels==1{vec![height,width]}else{vec![height,width,channels]},"scale":if depth||descriptor.starts_with("<f"){1.0}else if descriptor=="|u1"{255.0}else{intensity_scale},"units":if depth{"original recorded values"}else{"normalized intensity"}});
    crate::storage::write_json(
        &reuse_path,
        &json!({
            "source": provenance, "output": crate::storage::fingerprint(output)?, "tensor": tensor,
        }),
    )?;
    Ok(Some(tensor))
}
