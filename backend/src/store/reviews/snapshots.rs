//! Submitted content identities, scope validation and sample counting.
use super::*;

pub(super) fn validate_scope(recording: &Recording, scope: &Scope) -> Result<()> {
    let frames = recording.index["topics"][&scope.channel_id]["frames"]
        .as_array()
        .context("Scope topic not found")?;
    ensure!(
        scope.start_frame <= scope.end_frame && scope.end_frame < frames.len(),
        "Invalid review frame range"
    );
    Ok(())
}
pub(super) fn annotation_digest(
    recording: &Recording,
    scopes: &[Scope],
    dataset_id: Option<&str>,
) -> Result<String> {
    let mut data = json!({"sha256":recording.item["sha256"],"classes":recording.annotations["classes"],"class_uuids":recording.annotations["class_uuids"],"project_labels":recording.project["labels"],"project_label_lists":recording.project["label_lists"],"channels":{}});
    if let Some(id) = dataset_id {
        let mut dataset = recording.project["datasets"]
            .get(id)
            .context("Task dataset not found")?
            .clone();
        crate::storage::hydrate_dataset(&recording.folder, &mut dataset)?;
        data["dataset"] = dataset;
    }
    for scope in scopes {
        let channel = &scope.channel_id;
        let mut rows = serde_json::Map::new();
        for (number, row) in recording.annotations["annotations"][channel]
            .as_object()
            .into_iter()
            .flatten()
        {
            if scope.contains(channel, number.parse()?) {
                rows.insert(number.clone(), row.clone());
            }
        }
        let item = json!({"scope":scope,"rows":rows,"tracks":recording.annotations["tracks"][channel],"technique":recording.annotations["techniques"][channel],"label_list":recording.annotations["topic_label_lists"][channel]});
        data["channels"][format!("{channel}:{}:{}", scope.start_frame, scope.end_frame)] = item;
    }
    digest(&data)
}
pub(in crate::store) fn included(row: &Value) -> bool {
    row["reviewed"] == true
        || row["boxes"].as_array().is_some_and(|a| !a.is_empty())
        || row["labels"].as_array().is_some_and(|a| !a.is_empty())
        || row["fields"].as_object().is_some_and(|a| !a.is_empty())
}

pub(super) fn count_annotations(
    recording: &Recording,
    scopes: &[Scope],
    dataset_id: Option<&str>,
) -> Result<usize> {
    if let Some(id) = dataset_id {
        let mut dataset = recording.project["datasets"]
            .get(id)
            .context("Dataset not found")?
            .clone();
        crate::storage::hydrate_dataset(&recording.folder, &mut dataset)?;
        return Ok(dataset["entries"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, e)| {
                scopes.iter().any(|s| {
                    e["channel_id"] == s.channel_id
                        && e["frame"]
                            .as_u64()
                            .is_some_and(|n| s.contains(&s.channel_id, n as usize))
                })
            })
            .count());
    }
    Ok(scopes
        .iter()
        .map(|scope| {
            recording.annotations["annotations"][&scope.channel_id]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(n, r)| {
                    n.parse()
                        .is_ok_and(|n| scope.contains(&scope.channel_id, n))
                        && included(r)
                })
                .count()
        })
        .sum())
}
