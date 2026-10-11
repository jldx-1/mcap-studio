//! Submitted content is immutable. Version 3 reuses existing content-addressed
//! pages; legacy sources publish bounded immutable pages without copying MCAPs.
use super::*;
use crate::storage::{read_pages, write_changed, write_pages, PAGE_SIZE};
use std::collections::BTreeMap;

pub(super) fn publish_snapshot(recording: &Recording, task: &Task) -> Result<SnapshotRef> {
    let folder = &recording.folder;
    let recording_id = field(&recording.item, "uuid")?;
    let mut header = recording.annotations.clone();
    header
        .as_object_mut()
        .context("Invalid annotation state")?
        .remove("annotations");
    // View positions are working-copy preferences, not reviewed content.
    header.as_object_mut().unwrap().remove("settings");
    let original = contained_path(folder, &crate::storage::annotation_path(recording_id))?;
    let annotations_base = if recording.project["format_version"] == 3 && original.is_file() {
        let pages = read_json(&original)?["pages"].clone();
        let base = format!("reviews/{recording_id}/_pages/annotations");
        header["pages"] = pin_pages(
            folder,
            &format!("annotations/{recording_id}/pages"),
            &selected_pages(&pages, &task.scopes, false, recording_id)?,
            &base,
        )?;
        base
    } else {
        let base = format!("reviews/{recording_id}/_pages/annotations");
        let mut groups: BTreeMap<String, serde_json::Map<String, Value>> = BTreeMap::new();
        for scope in &task.scopes {
            for (number, row) in recording.annotations["annotations"][&scope.channel_id]
                .as_object()
                .into_iter()
                .flatten()
            {
                let frame = number.parse::<usize>()?;
                if scope.contains(&scope.channel_id, frame) {
                    groups
                        .entry(format!(
                            "{}/{:012}",
                            scope.channel_id,
                            frame as u64 / PAGE_SIZE
                        ))
                        .or_default()
                        .insert(format!("{}:{number}", scope.channel_id), row.clone());
                }
            }
        }
        header["pages"] = write_pages(folder, &base, groups)?;
        base
    };
    let mut datasets = serde_json::Map::new();
    for (id, dataset) in recording.project["datasets"]
        .as_object()
        .into_iter()
        .flatten()
    {
        if dataset["format"] != "csv"
            || dataset["source_recording_uuid"] != recording_id
            || !task
                .scopes
                .iter()
                .any(|s| dataset["source_channel"] == s.channel_id)
            || task
                .dataset_id
                .as_ref()
                .is_some_and(|selected| selected != id)
        {
            continue;
        }
        let mut metadata = dataset.clone();
        let base = if metadata["entry_pages"].is_object() {
            let base = format!("reviews/{recording_id}/_pages/datasets/{id}");
            metadata["entry_pages"] = pin_pages(
                folder,
                &format!("datasets/{id}/entries"),
                &selected_pages(&metadata["entry_pages"], &task.scopes, true, recording_id)?,
                &base,
            )?;
            base
        } else {
            let base = format!("reviews/{recording_id}/_pages/datasets/{id}");
            let mut groups: BTreeMap<String, serde_json::Map<String, Value>> = BTreeMap::new();
            for (key, entry) in metadata["entries"].as_object().into_iter().flatten() {
                let channel = field(entry, "channel_id")?;
                let number = entry["frame"].as_u64().context("Invalid CSV frame")?;
                if task
                    .scopes
                    .iter()
                    .any(|s| s.contains(channel, number as usize))
                {
                    groups
                        .entry(format!(
                            "{recording_id}/{channel}/{:012}",
                            number / PAGE_SIZE
                        ))
                        .or_default()
                        .insert(key.clone(), entry.clone());
                }
            }
            metadata["entry_pages"] = write_pages(folder, &base, groups)?;
            base
        };
        metadata.as_object_mut().unwrap().remove("entries");
        metadata.as_object_mut().unwrap().remove("overrides");
        datasets.insert(id.clone(), json!({"metadata":metadata,"pages_base":base}));
    }
    let source_dir = format!("reviews/{recording_id}/_source");
    let source_reference_path = contained_path(folder, &format!("{source_dir}/reference.json"))?;
    let reference = read_json(&source_reference_path).ok().filter(|value| {
        value["source_sha256"] == recording.item["sha256"]
            && value["index"]["path"]
                .as_str()
                .is_some_and(|p| contained_path(folder, p).is_ok_and(|p| p.is_file()))
    });
    let index_ref = if let Some(reference) = reference {
        reference["index"].clone()
    } else {
        let hash = digest(&recording.index)?;
        let path = format!("{source_dir}/{hash}.json");
        write_changed(&contained_path(folder, &path)?, &recording.index)?;
        let reference = json!({"path":path,"sha256":hash});
        write_json(
            &source_reference_path,
            &json!({"source_sha256":recording.item["sha256"],"index":reference}),
        )?;
        reference
    };
    let snapshot = json!({"kind":"mcap-review-snapshot","format_version":1,"project_uuid":recording.project["uuid"],"recording":recording.item,"scopes":task.scopes,"annotation_metadata":header,"annotation_pages_base":annotations_base,"datasets":datasets,"labels":recording.project["labels"],"label_lists":recording.project["label_lists"],"workflow":super::super::recording_workflow(recording),"index":index_ref});
    let sha256 = digest(&snapshot)?;
    let path = format!("reviews/{recording_id}/{}/snapshots/{sha256}.json", task.id);
    write_changed(&contained_path(folder, &path)?, &snapshot)?;
    Ok(SnapshotRef { path, sha256 })
}

fn pin_pages(
    folder: &Path,
    original_base: &str,
    pages: &Value,
    target_base: &str,
) -> Result<Value> {
    let mut pinned = serde_json::Map::new();
    for (group, page) in pages.as_object().context("Invalid snapshot pages")? {
        let hash = field(page, "sha256")?;
        let path = format!("{target_base}/{group}-{hash}.json");
        if !contained_path(folder, &path)?.is_file() {
            let rows = read_pages(folder, original_base, &json!({group:page}))?;
            write_pages(folder, target_base, BTreeMap::from([(group.clone(), rows)]))?;
        }
        let mut reference = page.clone();
        reference["path"] = json!(path);
        pinned.insert(group.clone(), reference);
    }
    Ok(json!(pinned))
}

fn selected_pages(
    pages: &Value,
    scopes: &[Scope],
    dataset: bool,
    recording: &str,
) -> Result<Value> {
    let mut selected = serde_json::Map::new();
    for (key, page) in pages.as_object().context("Invalid source pages")? {
        let parts: Vec<_> = key.split('/').collect();
        let (channel, number) = if dataset {
            ensure!(
                parts.len() == 3 && parts[0] == recording,
                "Invalid dataset page identity"
            );
            (parts[1], parts[2])
        } else {
            ensure!(parts.len() == 2, "Invalid annotation page identity");
            (parts[0], parts[1])
        };
        let bucket = number.parse::<u64>()?;
        if scopes.iter().any(|s| {
            s.channel_id == channel
                && s.start_frame as u64 / PAGE_SIZE <= bucket
                && s.end_frame as u64 / PAGE_SIZE >= bucket
        }) {
            selected.insert(key.clone(), page.clone());
        }
    }
    Ok(json!(selected))
}

pub(super) fn read_snapshot(folder: &Path, revision: &Revision) -> Result<Value> {
    ensure!(revision.snapshot_available,"This older review retained history but did not record an annotation snapshot; submit a new revision to capture the working copy");
    let expected = format!(
        "reviews/{}/{}/snapshots/{}.json",
        revision.recording_id, revision.task_id, revision.snapshot.sha256
    );
    ensure!(
        revision.snapshot.path == expected,
        "Revision snapshot path does not match identity"
    );
    let value = read_json(&contained_path(folder, &expected)?)?;
    ensure!(
        digest(&value)? == revision.snapshot.sha256
            && value["kind"] == "mcap-review-snapshot"
            && value["recording"]["uuid"] == revision.recording_id,
        "Revision snapshot hash does not match contents"
    );
    Ok(value)
}
pub(super) fn snapshot_annotations(folder: &Path, snapshot: &Value) -> Result<Value> {
    let mut state = snapshot["annotation_metadata"].clone();
    let rows = read_pages(
        folder,
        field(snapshot, "annotation_pages_base")?,
        &state["pages"],
    )?;
    state
        .as_object_mut()
        .context("Invalid snapshot annotations")?
        .remove("pages");
    let scopes: Vec<Scope> = serde_json::from_value(snapshot["scopes"].clone())?;
    state["annotations"] = json!({});
    state["settings"] = json!({"positions":{},"step":1});
    for (key, row) in rows {
        let (channel, number) = key
            .split_once(':')
            .context("Invalid snapshot annotation key")?;
        if scopes
            .iter()
            .any(|s| s.contains(channel, number.parse().unwrap_or(usize::MAX)))
        {
            if !state["annotations"][channel].is_object() {
                state["annotations"][channel] = json!({});
            }
            state["annotations"][channel][number] = row;
        }
    }
    Ok(state)
}
pub(super) fn snapshot_dataset(folder: &Path, snapshot: &Value, id: &str) -> Result<Value> {
    let saved = snapshot["datasets"]
        .get(id)
        .context("Revision dataset not found")?;
    let mut dataset = saved["metadata"].clone();
    let mut entries = read_pages(folder, field(saved, "pages_base")?, &dataset["entry_pages"])?;
    let scopes: Vec<Scope> = serde_json::from_value(snapshot["scopes"].clone())?;
    entries.retain(|_, entry| {
        entry["frame"].as_u64().is_some_and(|n| {
            scopes
                .iter()
                .any(|s| s.contains(entry["channel_id"].as_str().unwrap_or(""), n as usize))
        })
    });
    dataset["entries"] = json!(entries);
    Ok(dataset)
}
