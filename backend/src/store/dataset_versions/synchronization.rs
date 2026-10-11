//! Associate requested raw streams using the version's pinned timestamp recipe.
use super::*;

pub(super) fn frame_time(frame: &Value) -> Result<u64> {
    frame["time_ns"]
        .as_u64()
        .or_else(|| frame["time_ns"].as_str().and_then(|s| s.parse().ok()))
        .context("Frame requires an exact timestamp")
}
pub(super) fn input_references(
    index: &Value,
    source: &ApprovedSource,
    anchor_channel: &str,
    anchor_number: usize,
    selection: &Value,
) -> Result<Option<Value>> {
    let anchor = &index["topics"][anchor_channel]["frames"][anchor_number];
    let time = frame_time(anchor)?;
    let desired: Vec<String> = serde_json::from_value(selection["modalities"].clone())?;
    let reviewed = &source.snapshot["workflow"]["sync"];
    let requested = &selection["synchronization"];
    let mode = requested["mode"]
        .as_str()
        .or(reviewed["mode"].as_str())
        .unwrap_or("nearest");
    let mode = if mode == "basic" { "nearest" } else { mode };
    ensure!(
        ["nearest", "linear", "rate"].contains(&mode),
        "Choose nearest, linear, or rate synchronization"
    );
    let tolerance = requested["tolerance_ns"]
        .as_u64()
        .or(reviewed["tolerance_ns"].as_u64())
        .unwrap_or(100_000_000);
    let window = selection["sensor_window_ns"].as_u64().unwrap_or(0);
    ensure!(
        window <= 10_000_000_000,
        "Sensor windows must not exceed ten seconds"
    );
    let mut inputs = serde_json::Map::new();
    for kind in desired {
        let topics: Vec<_> = index["topics"]
            .as_object()
            .context("Invalid source index")?
            .iter()
            .filter(|(_, t)| {
                modality(t).0 == kind && t["frames"].as_array().is_some_and(|f| !f.is_empty())
            })
            .collect();
        if topics.is_empty() {
            return Ok(None);
        }
        let (anchor_kind, _) = modality(&index["topics"][anchor_channel]);
        let topics: Vec<_> =
            if index["topics"][anchor_channel]["kind"] == "image" && anchor_kind == kind {
                topics
                    .into_iter()
                    .filter(|(ch, _)| *ch == anchor_channel)
                    .collect()
            } else {
                topics
            };
        for (channel, topic) in &topics {
            let frames = topic["frames"].as_array().unwrap();
            let end = frames.partition_point(|f| frame_time(f).is_ok_and(|t| t <= time));
            let before = end.checked_sub(1);
            let after = (end < frames.len()).then_some(end);
            let nearest = match (before, after) {
                (Some(a), Some(b)) => {
                    if time.abs_diff(frame_time(&frames[a])?)
                        <= time.abs_diff(frame_time(&frames[b])?)
                    {
                        a
                    } else {
                        b
                    }
                }
                (Some(a), None) => a,
                (None, Some(b)) => b,
                _ => return Ok(None),
            };
            if time.abs_diff(frame_time(&frames[nearest])?) > tolerance {
                return Ok(None);
            }
            let key = if topics.len() == 1 {
                kind.clone()
            } else {
                format!("{kind}_{}", channel)
            };
            let mut input = json!({"modality":kind,"channel_id":channel,"topic":topic["topic"],"kind":topic["kind"],"frame":nearest,"time_ns":frames[nearest]["time_ns"],"source_project_uuid":source.project_id,"recording_id":source.task.recording_id,"source_sha256":source.snapshot["recording"]["sha256"],"image":frames[nearest]["image"],"width":frames[nearest]["width"],"height":frames[nearest]["height"],"source_frame":frames[nearest],"translation":source.snapshot["workflow"]["translations"][*channel],"synchronization":{"mode":mode,"tolerance_ns":tolerance,"target_time_ns":time}});
            if topic["kind"] != "image" && window > 0 {
                let low = time.saturating_sub(window / 2);
                let high = time.saturating_add(window - window / 2);
                let start = frames.partition_point(|f| frame_time(f).is_ok_and(|t| t < low));
                let end = frames.partition_point(|f| frame_time(f).is_ok_and(|t| t <= high));
                if start >= end {
                    return Ok(None);
                }
                ensure!(
                    end - start <= 4096,
                    "Sensor window contains more than 4096 records; choose a shorter window"
                );
                input["window"] =
                    json!({"start_frame":start,"end_frame":end-1,"duration_ns":window});
            } else if topic["kind"] != "image"
                && mode != "nearest"
                && frame_time(&frames[nearest])? != time
            {
                let pair = if mode == "rate" {
                    before.filter(|i| *i > 0).map(|b| (b - 1, b))
                } else {
                    before.zip(after)
                };
                if let Some((a, b)) = pair {
                    let left = frame_time(&frames[a])?;
                    let right = frame_time(&frames[b])?;
                    if right > left {
                        input["interpolation"] = json!({"left_frame":a,"right_frame":b,"numerator":(time as i128-left as i128).to_string(),"denominator":(right-left).to_string()});
                    }
                }
            }
            inputs.insert(key, input);
        }
    }
    Ok(Some(json!(inputs)))
}
