//! Annotation metadata and changed-page persistence.
use super::*;

pub(crate) fn annotation_buckets(changes: &Value) -> Result<std::collections::HashSet<String>> {
    changes
        .as_array()
        .context("Provide annotation changes")?
        .iter()
        .map(|change| {
            let channel = change["channel_id"]
                .as_str()
                .context("Missing channel ID")?;
            ensure!(
                channel.parse::<u16>()?.to_string() == channel,
                "Invalid channel ID"
            );
            let number = change["frame"].as_u64().context("Invalid frame ID")?;
            Ok(format!("{channel}/{:012}", number / PAGE_SIZE))
        })
        .collect()
}

pub(crate) fn read_annotation_subset(
    folder: &Path,
    id: &str,
    buckets: &std::collections::HashSet<String>,
) -> Result<Option<Value>> {
    let path = contained_path(folder, &annotation_path(id))?;
    if !path.is_file() {
        return Ok(None);
    }
    let mut state = read_json(&path)?;
    let mut pages = state
        .as_object_mut()
        .context("Annotation metadata must be an object")?
        .remove("pages")
        .context("Missing pages")?;
    pages
        .as_object_mut()
        .context("Invalid pages")?
        .retain(|key, _| buckets.contains(key));
    let mut channels: Map<String, Value> = Map::new();
    for (key, row) in read_pages(folder, &format!("annotations/{id}/pages"), &pages)? {
        let (channel, number) = key.split_once(':').context("Invalid annotation key")?;
        channels.entry(channel).or_insert_with(|| json!({}))[number] = row;
    }
    state["annotations"] = json!(channels);
    Ok(Some(state))
}

pub(crate) fn save_annotation_subset(
    folder: &Path,
    id: &str,
    state: &Value,
    buckets: &std::collections::HashSet<String>,
) -> Result<()> {
    let path = contained_path(folder, &annotation_path(id))?;
    let mut pages = if path.is_file() {
        read_json(&path)?["pages"].clone()
    } else {
        json!({})
    };
    let mut groups: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for (channel, rows) in state["annotations"]
        .as_object()
        .context("Missing annotations")?
    {
        for (number, row) in rows.as_object().context("Invalid annotations")? {
            let bucket = format!("{channel}/{:012}", number.parse::<u64>()? / PAGE_SIZE);
            if buckets.contains(&bucket) {
                groups
                    .entry(bucket)
                    .or_default()
                    .insert(format!("{channel}:{number}"), row.clone());
            }
        }
    }
    for bucket in buckets {
        pages
            .as_object_mut()
            .context("Invalid pages")?
            .remove(bucket);
    }
    let changed = write_pages(folder, &format!("annotations/{id}/pages"), groups)?;
    pages
        .as_object_mut()
        .unwrap()
        .extend(changed.as_object().unwrap().clone());
    let mut metadata = state.clone();
    metadata.as_object_mut().unwrap().remove("annotations");
    metadata["pages"] = pages;
    write_changed(&path, &metadata)
}

pub fn annotation_path(id: &str) -> String {
    format!("annotations/{id}/metadata.json")
}

pub fn read_annotations(folder: &Path, id: &str) -> Result<Option<Value>> {
    let path = contained_path(folder, &annotation_path(id))?;
    if !path.is_file() {
        return Ok(None);
    }
    let mut state = read_json(&path)?;
    let pages = state
        .as_object_mut()
        .context("Annotation metadata must be an object")?
        .remove("pages")
        .context("Missing annotation pages")?;
    let rows = read_pages(folder, &format!("annotations/{id}/pages"), &pages)?;
    let mut channels = Map::<String, Value>::new();
    for (key, row) in rows {
        let (channel, number) = key.split_once(':').context("Invalid annotation key")?;
        let channel = channels.entry(channel).or_insert_with(|| json!({}));
        channel[number] = row;
    }
    state["annotations"] = json!(channels);
    Ok(Some(state))
}

pub fn save_annotations(folder: &Path, id: &str, state: &Value) -> Result<()> {
    let mut groups: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for (channel, rows) in state["annotations"]
        .as_object()
        .context("Missing annotations")?
    {
        ensure!(
            channel.parse::<u16>()?.to_string() == *channel,
            "Invalid channel ID"
        );
        for (number, row) in rows.as_object().context("Invalid annotation rows")? {
            let frame: u64 = number.parse()?;
            ensure!(frame.to_string() == *number, "Invalid frame ID");
            groups
                .entry(format!("{channel}/{:012}", frame / PAGE_SIZE))
                .or_default()
                .insert(format!("{channel}:{number}"), row.clone());
        }
    }
    let mut metadata = state.clone();
    metadata.as_object_mut().unwrap().remove("annotations");
    metadata["pages"] = write_pages(folder, &format!("annotations/{id}/pages"), groups)?;
    write_changed(&contained_path(folder, &annotation_path(id))?, &metadata)
}
