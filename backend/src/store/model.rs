//! Shared JSON field access, identifiers and browser serialization.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use uuid::Uuid;

pub(super) fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .with_context(|| format!("Missing {name}"))
}

pub(super) fn registry<'a>(manifest: &'a Value, name: &str) -> Result<&'a Map<String, Value>> {
    manifest[name]
        .as_object()
        .with_context(|| format!("Project {name} must be a UUID dictionary"))
}

pub(super) fn canonical_uuid(value: &str) -> Result<()> {
    ensure!(
        Uuid::parse_str(value)?.to_string() == value,
        "Expected a canonical UUID"
    );
    Ok(())
}

pub(super) fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

// Browsers serialize 1.0 as 1. Compare JSON numeric values while keeping large
// integer timestamps exact instead of rounding both operands through f64.
pub(super) fn same_json(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, value)| right.get(key).is_some_and(|other| same_json(value, other)))
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_json(left, right))
        }
        (Value::Number(left), Value::Number(right)) if left.is_f64() != right.is_f64() => {
            let (float, integer) = if left.is_f64() {
                (left, right)
            } else {
                (right, left)
            };
            let value = float.as_f64().unwrap();
            if value.fract() != 0.0 {
                return false;
            }
            if let Some(integer) = integer.as_u64() {
                value >= 0.0 && value < (u64::MAX as f64) && value as u64 == integer
            } else if let Some(integer) = integer.as_i64() {
                value >= (i64::MIN as f64) && value < (i64::MAX as f64) && value as i64 == integer
            } else {
                false
            }
        }
        _ => left == right,
    }
}

pub(super) fn valid_name(value: &Value) -> Result<String> {
    let name = field(value, "name")?.trim();
    ensure!(
        !name.is_empty() && name.len() <= 200 && !name.chars().any(char::is_control),
        "Enter a nonempty name of at most 200 bytes"
    );
    Ok(name.to_owned())
}

/// Browser numbers cannot represent nanoseconds precisely; disk values remain u64.
pub fn stringify_timestamps(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object.iter_mut() {
                if key.ends_with("_ns") {
                    if let Some(number) = value.as_u64() {
                        *value = json!(number.to_string());
                    }
                } else {
                    stringify_timestamps(value);
                }
            }
        }
        Value::Array(array) => {
            for value in array {
                stringify_timestamps(value);
            }
        }
        _ => {}
    }
}
