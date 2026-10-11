//! Camera payload decoding, pixel formats, stride and endianness.
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{DynamicImage, RgbImage};
use serde_json::Value;

pub(super) fn payload_bytes(value: &Value) -> Result<Vec<u8>> {
    if let Some(encoded) = value
        .as_str()
        .or_else(|| value.get("$bytes_base64").and_then(Value::as_str))
    {
        return STANDARD
            .decode(encoded)
            .context("Image data contains invalid base64");
    }
    if let Some(array) = value.as_array() {
        return array
            .iter()
            .map(|item| {
                item.as_u64()
                    .filter(|&v| v <= 255)
                    .map(|v| v as u8)
                    .context("Image byte array must contain integers between 0 and 255")
            })
            .collect();
    }
    bail!("Image data must be base64, a $bytes_base64 object, or a byte array")
}

pub(super) fn is_camera(record: &Value) -> bool {
    record.get("data").is_some()
        && ["width", "format", "encoding", "pixel_format"]
            .iter()
            .any(|key| record.get(*key).is_some())
}

pub(super) fn decode_image(record: &Value) -> Result<DynamicImage> {
    let bytes = payload_bytes(&record["data"])?;
    if bytes.starts_with(b"\x89PNG") || bytes.starts_with(b"\xff\xd8") {
        return image::load_from_memory(&bytes)
            .map(|image| DynamicImage::ImageRgb8(image.to_rgb8()))
            .context("Invalid PNG/JPEG image");
    }
    let format = record["format"].as_str().unwrap_or("").to_ascii_lowercase();
    ensure!(!matches!(format.as_str(), "h264" | "h265" | "hevc" | "vp9" | "av1"),
        "Compressed video ({format}) is not supported by the native image decoder; import PNG/JPEG or raw images");
    let encoding = record
        .get("pixel_format")
        .or_else(|| record.get("encoding"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let (channels, sample_bytes, reverse, float) = match encoding.as_str() {
        "rgb8" => (3, 1, false, false), "bgr8" | "8uc3" => (3, 1, true, false),
        "rgba8" => (4, 1, false, false), "bgra8" | "8uc4" => (4, 1, true, false),
        "rgb16" => (3, 2, false, false), "bgr16" => (3, 2, true, false),
        "rgba16" => (4, 2, false, false), "bgra16" => (4, 2, true, false),
        "mono8" | "mono_8" | "8uc1" => (1, 1, false, false),
        "mono16" | "mono_16" | "16uc1" => (1, 2, false, false),
        "32fc1" | "mono32f" => (1, 4, false, true),
        "64fc1" | "mono64f" => (1, 8, false, true),
        _ => bail!("Unsupported raw image encoding {encoding}; use RGB/BGR/RGBA/BGRA 8/16-bit, mono8/16, or mono32f/64f"),
    };
    let width = record["width"]
        .as_u64()
        .context("Raw image width must be an integer")?;
    let height = record["height"]
        .as_u64()
        .context("Raw image height must be an integer")?;
    ensure!(
        width > 0
            && height > 0
            && width <= u32::MAX as u64
            && height <= u32::MAX as u64
            && width.checked_mul(height).is_some_and(|v| v <= 100_000_000),
        "Invalid or excessive raw image dimensions"
    );
    let row_bytes = width as usize * channels * sample_bytes;
    let stride = match record.get("step") {
        Some(value) => value.as_u64().context("Image step must be an integer")? as usize,
        None => row_bytes,
    };
    ensure!(
        stride >= row_bytes
            && (height as usize)
                .checked_mul(stride)
                .is_some_and(|size| size <= bytes.len()),
        "Image payload/stride does not match dimensions"
    );
    let big_endian = record["is_bigendian"]
        .as_bool()
        .unwrap_or_else(|| record["is_bigendian"].as_u64() == Some(1));
    let read_sample = |offset: usize| -> f64 {
        let slice = &bytes[offset..offset + sample_bytes];
        match (sample_bytes, float) {
            (1, _) => slice[0] as f64,
            (2, _) => {
                let raw = [slice[0], slice[1]];
                (if big_endian {
                    u16::from_be_bytes(raw)
                } else {
                    u16::from_le_bytes(raw)
                }) as f64
            }
            (4, true) => {
                let raw: [u8; 4] = slice.try_into().unwrap();
                (if big_endian {
                    f32::from_be_bytes(raw)
                } else {
                    f32::from_le_bytes(raw)
                }) as f64
            }
            (8, true) => {
                let raw: [u8; 8] = slice.try_into().unwrap();
                if big_endian {
                    f64::from_be_bytes(raw)
                } else {
                    f64::from_le_bytes(raw)
                }
            }
            _ => unreachable!(),
        }
    };
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    if float {
        for y in 0..height as usize {
            for x in 0..width as usize {
                let value = read_sample(y * stride + x * sample_bytes);
                if value.is_finite() {
                    low = low.min(value);
                    high = high.max(value);
                }
            }
        }
        if low >= 0.0 && high <= 1.0 {
            low = 0.0;
            high = 1.0;
        }
    }
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height as usize {
        for x in 0..width as usize {
            for component in 0..3 {
                let channel = if channels == 1 {
                    0
                } else if reverse {
                    2 - component
                } else {
                    component
                };
                let value = read_sample(y * stride + (x * channels + channel) * sample_bytes);
                let display = if float {
                    if !value.is_finite() || !low.is_finite() || high == low {
                        0.0
                    } else {
                        (value - low) * 255.0 / (high - low)
                    }
                } else if sample_bytes == 2 {
                    value / 257.0
                } else {
                    value
                };
                rgb.push(display.clamp(0.0, 255.0) as u8);
            }
        }
    }
    Ok(DynamicImage::ImageRgb8(
        RgbImage::from_raw(width as u32, height as u32, rgb).context("Invalid RGB image")?,
    ))
}
