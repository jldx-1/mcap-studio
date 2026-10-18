//! Stateful decoding of Annex B camera packets, isolated by MCAP channel.
use anyhow::{ensure, Context, Result};
use image::{DynamicImage, RgbImage};
use openh264::{decoder::Decoder, formats::YUVSource};
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn compressed_format(record: &Value) -> Option<&'static str> {
    for field in ["format", "codec", "encoding", "pixel_format"] {
        let Some(value) = record[field].as_str() else {
            continue;
        };
        match value.trim().to_ascii_lowercase().as_str() {
            "h264" | "h.264" | "avc" | "avc1" | "video/h264" | "video/avc" => return Some("h264"),
            "jpeg" | "jpg" | "mjpeg" | "mjpg" | "image/jpeg" | "video/jpeg" | "video/mjpeg" => {
                return Some("jpeg")
            }
            "png" | "image/png" => return Some("png"),
            _ => {}
        }
    }
    None
}

#[derive(Default)]
pub(super) struct CameraDecoder {
    h264: HashMap<u16, Decoder>,
}
impl CameraDecoder {
    pub fn decode(
        &mut self,
        channel: u16,
        record: &Value,
        bytes: &[u8],
    ) -> Result<Option<DynamicImage>> {
        if compressed_format(record) != Some("h264") {
            return super::images::decode_image_bytes(record, bytes).map(Some);
        }
        ensure!(
            bytes.len() <= i32::MAX as usize,
            "H.264 packet is too large"
        );
        ensure!(bytes.starts_with(&[0, 0, 1]) || bytes.starts_with(&[0, 0, 0, 1]),
            "H.264 requires Annex B start codes, with one complete video frame per MCAP message; convert length-prefixed AVC/MP4 packets before import");
        let mut pictures = 0;
        for nal in openh264::nal_units(bytes) {
            // nal_units returns a three-byte start code, including when the
            // original stream used four bytes. Strip only the prefix.
            let header = *nal.get(3).context("Incomplete H.264 NAL unit")?;
            if matches!(header & 31, 1 | 5) {
                let mut bits = SliceBits::new(&nal[4..]);
                let first_mb = bits.unsigned_exp_golomb()?;
                let slice_type = bits.unsigned_exp_golomb()?;
                ensure!(slice_type % 5 != 1, "H.264 B-frames are not supported; record Annex B video without B-frames (Foxglove CompressedVideo layout)");
                pictures += usize::from(first_mb == 0);
            }
        }
        ensure!(pictures <= 1, "Each H.264 MCAP message must contain exactly one complete video frame, not a whole video file");
        let decoder = match self.h264.entry(channel) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Decoder::new().context("Cannot initialize the bundled H.264 decoder")?)
            }
        };
        let output = decoder.decode(bytes).context("Invalid or incomplete H.264 packet; a stream must start with SPS/PPS headers and an IDR keyframe")?;
        match output {
            Some(frame) => {
                ensure!(
                    pictures == 1,
                    "H.264 output has no matching frame timestamp"
                );
                let (width, height) = frame.dimensions();
                ensure!(
                    width > 0
                        && height > 0
                        && width
                            .checked_mul(height)
                            .is_some_and(|pixels| pixels <= 100_000_000),
                    "Invalid or excessive H.264 image dimensions"
                );
                let mut rgb = vec![0; width * height * 3];
                frame.write_rgb8(&mut rgb);
                Ok(Some(DynamicImage::ImageRgb8(
                    RgbImage::from_raw(width as u32, height as u32, rgb)
                        .context("Invalid decoded H.264 image")?,
                )))
            }
            None => {
                ensure!(pictures == 0, "H.264 packet did not yield a complete frame; include SPS/PPS headers and begin with an IDR keyframe");
                Ok(None) // SPS/PPS-only messages configure the decoder, not frames.
            }
        }
    }
}

// Only the first two unsigned Exp-Golomb fields are needed to distinguish
// frames, slices, and B-frames. Remove emulation prevention before reading bits.
struct SliceBits {
    bytes: Vec<u8>,
    bit: usize,
}
impl SliceBits {
    fn new(bytes: &[u8]) -> Self {
        let mut rbsp = Vec::new();
        let mut zeros = 0;
        // Header fields are bounded; no need to copy an entire compressed frame.
        for &byte in bytes.iter().take(64) {
            if zeros >= 2 && byte == 3 {
                zeros = 0;
                continue;
            }
            rbsp.push(byte);
            zeros = if byte == 0 { zeros + 1 } else { 0 };
        }
        Self {
            bytes: rbsp,
            bit: 0,
        }
    }
    fn bit(&mut self) -> Result<u32> {
        let byte = self
            .bytes
            .get(self.bit / 8)
            .context("Incomplete H.264 slice header")?;
        let value = (byte >> (7 - self.bit % 8)) & 1;
        self.bit += 1;
        Ok(value as u32)
    }
    fn unsigned_exp_golomb(&mut self) -> Result<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            ensure!(zeros < 32, "Invalid H.264 slice header");
        }
        let mut value = 1;
        for _ in 0..zeros {
            value = (value << 1) | self.bit()?;
        }
        Ok(value - 1)
    }
}
