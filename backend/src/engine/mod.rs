//! Native MCAP engine. Import/index, decoding and processing have focused modules.
use anyhow::{bail, Context, Result};
use serde_json::{json, Number, Value};

mod decoder;
mod images;
mod index;
mod processing;
mod reader;
mod synchronization;
mod training_images;
mod translation;
mod translation_files;
mod translation_stream;
pub use training_images::{ensure_native_training_images, write_training_tensor};
pub use translation_files::{translation_channels, translation_file_script, translation_sample};

use decoder::Decoder;
use images::{decode_image, is_camera};
pub use index::{build_index, build_index_and_hash};
pub use processing::process_recording;
use reader::stream_messages;
pub use translation::translate_record;
pub(crate) use translation::CompiledTranslation;
pub fn interpolate_records(
    left: &Value,
    right: &Value,
    fallback: &Value,
    numerator: i128,
    denominator: i128,
) -> Result<Value> {
    synchronization::interpolate(
        left,
        right,
        fallback,
        (numerator, denominator),
        "",
        "time_ns",
    )
}

fn finite_number(value: f64) -> Result<Value> {
    Number::from_f64(value)
        .map(Value::Number)
        .context("Nonfinite numbers cannot be represented in JSON")
}

fn integer(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
}

fn integer_value(value: i128) -> Result<Value> {
    if let Ok(value) = i64::try_from(value) {
        Ok(json!(value))
    } else if let Ok(value) = u64::try_from(value) {
        Ok(json!(value))
    } else {
        bail!("Integer arithmetic exceeds the 64-bit JSON range")
    }
}

#[cfg(test)]
mod tests;
