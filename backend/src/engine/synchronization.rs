//! Clock selection, interpolation, resampling and synchronization validation.
use anyhow::{ensure, Context, Result};
use serde_json::{Map, Value};

use super::translation::get_path;
use super::{finite_number, integer, integer_value};

#[derive(Clone, Debug)]
pub(super) struct Sample {
    pub(super) ordinal: usize,
    pub(super) time: u64,
    pub(super) log_time: u64,
    pub(super) publish_time: u64,
    pub(super) sequence: u32,
    pub(super) record: Value,
}

pub(super) fn channel_id(value: &Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|id| u16::try_from(id).ok())
        .or_else(|| value.as_str().and_then(|id| id.parse().ok()))
}

pub(super) fn timestamp(
    record: &Value,
    field: &str,
    log_time: u64,
    publish_time: u64,
) -> Result<u64> {
    match field {
        "MCAP log time" | "log_time" | "" => Ok(log_time),
        "MCAP publish time" | "publish_time" => Ok(publish_time),
        _ => {
            let value = get_path(record, field)?;
            if let Some(value) = value.as_u64() {
                return Ok(value);
            }
            let seconds = integer(&value["seconds"])
                .context("Timestamp requires integer nanoseconds or seconds/nanos")?;
            let nanos = value["nanos"]
                .as_u64()
                .filter(|&v| v < 1_000_000_000)
                .context("Timestamp nanos must be 0..999999999")?;
            u64::try_from(seconds * 1_000_000_000 + nanos as i128)
                .context("Timestamp is outside the MCAP clock range")
        }
    }
}

pub(super) fn interpolate(
    left: &Value,
    right: &Value,
    fallback: &Value,
    ratio: (i128, i128),
    path: &str,
    clock: &str,
) -> Result<Value> {
    if path == clock {
        return Ok(fallback.clone());
    }
    if let (Some(a), Some(b)) = (integer(left), integer(right)) {
        return interpolate_integer(a, b, ratio);
    }
    let fraction = ratio.0 as f64 / ratio.1 as f64;
    if let (Some(a), Some(b)) = (left.as_f64(), right.as_f64()) {
        return finite_number(a + (b - a) * fraction);
    }
    if let (Some(a), Some(b), Some(base)) = (left.as_array(), right.as_array(), fallback.as_array())
    {
        if a.len() == b.len() && a.len() == base.len() {
            return Ok(Value::Array(
                a.iter()
                    .zip(b)
                    .zip(base)
                    .map(|((a, b), base)| interpolate(a, b, base, ratio, path, clock))
                    .collect::<Result<_>>()?,
            ));
        }
    }
    if let (Some(a), Some(b), Some(base)) =
        (left.as_object(), right.as_object(), fallback.as_object())
    {
        if a.len() == b.len()
            && a.len() == base.len()
            && a.keys()
                .all(|key| b.contains_key(key) && base.contains_key(key))
        {
            let mut result = Map::new();
            for (key, value) in a {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                result.insert(
                    key.clone(),
                    interpolate(value, &b[key], &base[key], ratio, &child, clock)?,
                );
            }
            return Ok(Value::Object(result));
        }
    }
    Ok(fallback.clone())
}

pub(super) fn interpolate_integer(
    origin: i128,
    endpoint: i128,
    ratio: (i128, i128),
) -> Result<Value> {
    ensure!(
        ratio.1 > 0,
        "Interpolation requires distinct source timestamps"
    );
    // Divide before multiplication, then round the complete result to the
    // nearest even integer, exactly as the Python Fraction implementation does.
    // The integer origin never passes through a float (including ns clocks).
    let delta = endpoint - origin;
    let denominator = ratio.1;
    let whole = (delta / denominator)
        .checked_mul(ratio.0)
        .context("Integer interpolation overflow")?;
    let residual = delta % denominator;
    // The residual and timestamp numerator each fit u64; their product fits
    // u128 even when it would overflow signed i128.
    let residual_product = residual
        .unsigned_abs()
        .checked_mul(ratio.0.unsigned_abs())
        .context("Integer interpolation overflow")?;
    let sign = residual.signum() * ratio.0.signum();
    let quotient = i128::try_from(residual_product / denominator as u128)
        .context("Integer interpolation overflow")?
        * sign;
    let mut result = origin
        .checked_add(whole)
        .and_then(|result| result.checked_add(quotient))
        .context("Integer interpolation overflow")?;
    let remainder = (residual_product % denominator as u128) as i128 * sign;
    let twice_remainder = remainder
        .unsigned_abs()
        .checked_mul(2)
        .context("Integer interpolation overflow")?;
    if twice_remainder > denominator as u128
        || (twice_remainder == denominator as u128 && result % 2 != 0)
    {
        result = result
            .checked_add(remainder.signum())
            .context("Integer interpolation overflow")?;
    }
    integer_value(result)
}

pub(super) fn resample(
    samples: &[Sample],
    time: u64,
    options: &Value,
) -> Result<Option<(Sample, Vec<u64>)>> {
    validate_sync_options(options)?;
    if samples.is_empty() {
        return Ok(None);
    }
    let index = samples.partition_point(|sample| sample.time < time);
    if index < samples.len() && samples[index].time == time {
        return Ok(Some((samples[index].clone(), vec![time])));
    }
    let left = index.saturating_sub(1);
    let right = index.min(samples.len() - 1);
    let nearest = if samples[left].time.abs_diff(time) <= samples[right].time.abs_diff(time) {
        left
    } else {
        right
    };
    let mode = options["mode"].as_str().unwrap_or("basic");
    ensure!(
        matches!(
            mode,
            "basic" | "nearest" | "exact" | "linear" | "interpolate" | "rate"
        ),
        "Unknown synchronization mode {mode}"
    );
    let tolerance = match options.get("tolerance_ns") {
        Some(value) if !value.is_null() => Some(
            value
                .as_u64()
                .context("Tolerance must be nonnegative integer nanoseconds")?,
        ),
        _ => None,
    };
    if mode == "exact"
        || tolerance.is_some_and(|tolerance| samples[nearest].time.abs_diff(time) > tolerance)
    {
        return Ok(None);
    }
    if matches!(mode, "basic" | "nearest") {
        return Ok(Some((
            samples[nearest].clone(),
            vec![samples[nearest].time],
        )));
    }
    if samples.len() < 2 {
        return Ok(None);
    }
    let end = if mode == "rate" {
        index.saturating_sub(1).clamp(1, samples.len() - 1)
    } else {
        index.clamp(1, samples.len() - 1)
    };
    let (a, b) = (&samples[end - 1], &samples[end]);
    let ratio = (time as i128 - a.time as i128, (b.time - a.time) as i128);
    let mut sample = samples[nearest].clone();
    sample.record = interpolate(
        &a.record,
        &b.record,
        &sample.record,
        ratio,
        "",
        options["timestamp_field"]
            .as_str()
            .unwrap_or("MCAP log time"),
    )?;
    Ok(Some((sample, vec![a.time, b.time, samples[nearest].time])))
}

pub(super) fn validate_sync_options(options: &Value) -> Result<()> {
    let mode = options["mode"].as_str().unwrap_or("basic");
    ensure!(
        matches!(
            mode,
            "basic" | "nearest" | "exact" | "linear" | "interpolate" | "rate"
        ),
        "Unknown synchronization mode {mode}"
    );
    if let Some(tolerance) = options.get("tolerance_ns").filter(|value| !value.is_null()) {
        ensure!(
            tolerance.as_u64().is_some(),
            "Tolerance must be nonnegative integer nanoseconds"
        );
    }
    Ok(())
}
