//! Bounded sample publication reads immutable annotation pages and metadata only.
use super::synchronization::{frame_time, input_references};
use super::*;

pub(super) struct Samples {
    pub pages: Value,
    pub count: u64,
    pub dropped: u64,
    pub images: bool,
    pub classifications: bool,
    pub columns: Vec<String>,
}
pub(super) fn class_id(source: &ApprovedSource, class: usize) -> String {
    source.snapshot["annotation_metadata"]["class_uuids"][class]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("legacy:{}:{class}", source.task.recording_id))
}
pub(super) fn version_labels(
    sources: &[ApprovedSource],
    wanted: &BTreeSet<String>,
) -> Result<Value> {
    let mut labels: BTreeMap<String, Value> = BTreeMap::new();
    for source in sources {
        for (n, name) in source.snapshot["annotation_metadata"]["classes"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let id = class_id(source, n);
            if wanted.is_empty() || wanted.contains(&id) {
                labels
                    .entry(id.clone())
                    .or_insert_with(|| json!({"id":id,"name":name}));
            }
        }
        for (id, label) in source.snapshot["labels"].as_object().into_iter().flatten() {
            if wanted.is_empty() || wanted.contains(id) {
                labels
                    .entry(id.clone())
                    .or_insert_with(|| json!({"id":id,"name":label["name"]}));
            }
        }
    }
    ensure!(
        wanted.iter().all(|id| labels.contains_key(id)),
        "A selected label is absent from the approved source definitions"
    );
    Ok(json!(labels.into_values().collect::<Vec<_>>()))
}
fn remap_row(
    source: &ApprovedSource,
    row: &Value,
    labels: &Value,
    wanted: &BTreeSet<String>,
) -> Result<Option<Value>> {
    let mut result = row.clone();
    let mut matched = false;
    let mut boxes = vec![];
    let mut classifications = vec![];
    for bbox in row["boxes"].as_array().into_iter().flatten() {
        let original = bbox["class_id"]
            .as_u64()
            .context("Invalid approved box class")? as usize;
        let id = class_id(source, original);
        if wanted.is_empty() || wanted.contains(&id) {
            let class = labels
                .as_array()
                .unwrap()
                .iter()
                .position(|l| l["id"] == id)
                .context("Approved class missing from version definitions")?;
            let mut bbox = bbox.clone();
            bbox["class_id"] = json!(class);
            boxes.push(bbox);
            matched = true;
        }
    }
    for id in row["labels"].as_array().into_iter().flatten() {
        let original = id.as_u64().context("Invalid classification class")? as usize;
        let id = class_id(source, original);
        if wanted.is_empty() || wanted.contains(&id) {
            let class = labels
                .as_array()
                .unwrap()
                .iter()
                .position(|l| l["id"] == id)
                .context("Approved class missing from version")?;
            classifications.push(json!(class));
            matched = true;
        }
    }
    if !wanted.is_empty() && !matched {
        return Ok(None);
    }
    result["boxes"] = json!(boxes);
    result["labels"] = json!(classifications);
    Ok(Some(result))
}
pub(super) fn select_samples(
    store: &Store,
    folder: &Path,
    base: &str,
    sources: &[ApprovedSource],
    selection: &Value,
    labels: &Value,
) -> Result<Samples> {
    let wanted: BTreeSet<String> = serde_json::from_value(selection["label_ids"].clone())?;
    let primary = selection["primary_modality"].as_str();
    let mut pages = serde_json::Map::new();
    let mut group = serde_json::Map::new();
    let mut count = 0u64;
    let mut dropped = 0u64;
    let mut images = false;
    let mut classifications = false;
    let mut columns = BTreeSet::new();
    {
        let mut push = |source: &ApprovedSource,
                        index: &Value,
                        channel: &str,
                        number: usize,
                        row: Value,
                        values: Option<Value>|
         -> Result<()> {
            if !source
                .revision
                .scopes
                .iter()
                .any(|s| s.contains(channel, number))
            {
                return Ok(());
            }
            if let Some(primary) = primary {
                if modality(&index["topics"][channel]).0 != primary {
                    return Ok(());
                }
            }
            let negative = row["boxes"].as_array().is_none_or(Vec::is_empty)
                && row["labels"].as_array().is_none_or(Vec::is_empty);
            if negative && values.is_none() && selection["include_negatives"] == false {
                return Ok(());
            }
            let Some(row) = remap_row(source, &row, labels, &wanted)? else {
                return Ok(());
            };
            let Some(inputs) = input_references(index, source, channel, number, selection)? else {
                dropped += 1;
                return Ok(());
            };
            let frame = &index["topics"][channel]["frames"][number];
            let _ = frame_time(frame)?;
            images |= inputs.as_object().unwrap().values().any(|i| {
                i["kind"] == "image" && i["channel_id"] == channel && i["frame"] == number
            });
            classifications |= row["labels"].as_array().is_some_and(|l| !l.is_empty());
            if let Some(values) = &values {
                for key in values.as_object().into_iter().flatten().map(|(k, _)| k) {
                    columns.insert(key.clone());
                }
            }
            let primary_input = inputs
                .as_object()
                .unwrap()
                .iter()
                .find(|(_, i)| i["channel_id"] == channel && i["frame"] == number)
                .map(|(k, _)| k.clone());
            let mut sample = json!({"key":format!("{count:016}"),"source_project_uuid":source.project_id,"mcap_uuid":source.task.recording_id,"channel_id":channel,"frame":number,"time_ns":frame["time_ns"],"width":frame["width"],"height":frame["height"],"approved_revision":source.reference(),"primary_input":primary_input,"annotation":row,"inputs":inputs});
            if let Some(values) = values {
                sample["values"] = values;
            }
            group.insert(format!("{count:016}"), sample);
            count += 1;
            if group.len() >= PAGE_SIZE as usize {
                let bucket = format!("{:012}", (count - 1) / PAGE_SIZE);
                let groups = BTreeMap::from([(bucket, std::mem::take(&mut group))]);
                pages.extend(
                    write_pages(folder, &format!("{base}/samples"), groups)?
                        .as_object()
                        .unwrap()
                        .clone(),
                );
            }
            Ok(())
        };
        for source in sources {
            let index = store.shared_json(&contained_path(
                &source.folder,
                field(&source.snapshot["index"], "path")?,
            )?)?;
            let scopes = &source.revision.scopes;
            let header = &source.snapshot["annotation_metadata"];
            if source.task.dataset_id.is_none() {
                for (bucket, page) in header["pages"]
                    .as_object()
                    .context("Invalid annotation snapshot pages")?
                {
                    let rows = read_pages(
                        &source.folder,
                        field(&source.snapshot, "annotation_pages_base")?,
                        &json!({bucket:page}),
                    )?;
                    for (key, row) in rows {
                        let (channel, n) = key
                            .split_once(':')
                            .context("Invalid annotation snapshot key")?;
                        let n = n.parse::<usize>()?;
                        if scopes.iter().any(|s| s.contains(channel, n))
                            && super::super::reviews::included(&row)
                        {
                            push(source, &index, channel, n, row, None)?;
                        }
                    }
                }
            }
            // A dedicated CSV task uses its frozen column values as the target work.
            if let Some(dataset) = &source.task.dataset_id {
                let saved = &source.snapshot["datasets"][dataset];
                let metadata = &saved["metadata"];
                for (bucket, page) in metadata["entry_pages"]
                    .as_object()
                    .context("Missing approved CSV pages")?
                {
                    let rows = read_pages(
                        &source.folder,
                        field(saved, "pages_base")?,
                        &json!({bucket:page}),
                    )?;
                    for entry in rows.values() {
                        let channel = field(entry, "channel_id")?;
                        let number = entry["frame"]
                            .as_u64()
                            .context("Invalid approved CSV frame")?
                            as usize;
                        let mut classes = vec![];
                        for column in metadata["columns"].as_array().into_iter().flatten() {
                            let Some(list) = column["label_list"].as_str() else {
                                continue;
                            };
                            let value = &entry["values"][column["name"].as_str().unwrap_or("")];
                            for id in source.snapshot["label_lists"][list]["label_ids"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                            {
                                if value == &source.snapshot["labels"][id]["name"] {
                                    if let Some(position) = header["class_uuids"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .position(|label| label == id)
                                    {
                                        classes.push(json!(position));
                                    }
                                }
                            }
                        }
                        let row = json!({"reviewed":true,"boxes":[],"labels":classes});
                        push(
                            source,
                            &index,
                            channel,
                            number,
                            row,
                            Some(entry["values"].clone()),
                        )?;
                    }
                }
            }
        }
    }
    if !group.is_empty() {
        let bucket = format!("{:012}", (count - 1) / PAGE_SIZE);
        pages.extend(
            write_pages(
                folder,
                &format!("{base}/samples"),
                BTreeMap::from([(bucket, group)]),
            )?
            .as_object()
            .unwrap()
            .clone(),
        );
    }
    Ok(Samples {
        pages: json!(pages),
        count,
        dropped,
        images,
        classifications,
        columns: columns.into_iter().collect(),
    })
}
