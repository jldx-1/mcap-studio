//! Rebuildable CSV and YOLO materializations with per-sample dependencies.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Write,
    path::Path,
};

use super::{
    contained_path, csv_cell, field, indexed_frame, load_manifest, read_json, recording_workflow,
    registry, validate_annotations, Store,
};
use crate::storage::{
    digest, fingerprint, hydrate_dataset, read_pages, write_changed, write_pages, PAGE_SIZE,
};

fn write_text_changed(path: &Path, bytes: &[u8]) -> Result<bool> {
    if path.is_file() && fs::read(path)? == bytes {
        return Ok(false);
    }
    fs::create_dir_all(path.parent().context("Missing output directory")?)?;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(true)
}

impl Store {
    /// Add format implementations here without changing source membership storage.
    pub fn build_dataset(&self, id: &str, dataset_id: &str, request: &Value) -> Result<Value> {
        let (_, metadata) = self.manifest(id)?;
        if metadata["datasets"][dataset_id]["kind"] == "approved-dataset" {
            let version = request["version"]
                .as_u64()
                .or(metadata["datasets"][dataset_id]["latest_version"].as_u64())
                .context("Dataset has no version")?;
            return self.build_dataset_version(id, dataset_id, version, request);
        }
        ensure!(self.accounts.setup_required()? || !self.accounts.settings()?.require_approval_for_datasets,
            "This legacy dataset has no pinned revision versions; create a dataset version from Approved data before building production training data");
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let manifest = load_manifest(&folder)?;
        Self::editable(&manifest)?;
        let mut dataset = manifest["datasets"]
            .get(dataset_id)
            .cloned()
            .context("Dataset not found")?;
        hydrate_dataset(&folder, &mut dataset)?;
        let format = request["format"]
            .as_str()
            .or(dataset["format"].as_str())
            .unwrap_or("yolo");
        ensure!(matches!(format, "yolo" | "csv"), "Choose csv or yolo");
        ensure!(
            format != "yolo" || dataset["format"] != "csv",
            "Structured CSV datasets cannot be generated as YOLO"
        );
        let base = format!("datasets/{dataset_id}/generated/{format}");
        let output = contained_path(&folder, &base)?;
        fs::create_dir_all(&output)?;
        let index_path = contained_path(&folder, &format!("{base}/manifest.json"))?;
        let previous = read_json(&index_path).ok();
        let old = if let Some(previous) = &previous {
            read_pages(
                &folder,
                &format!("{base}/samples"),
                &previous["sample_pages"],
            )
            .unwrap_or_default()
        } else {
            Map::new()
        };
        let mut classes: Vec<Value> = Vec::new();
        let mut identities: Vec<Value> = Vec::new();
        for (uuid, label) in registry(&manifest, "labels")? {
            identities.push(json!(uuid));
            classes.push(label["name"].clone());
        }
        // Legacy, unnamed project classes are retained after UUID-backed classes.
        let mut recordings = HashMap::new();
        let mut workflows = HashMap::new();
        let mut approved = HashMap::new();
        let require_approval = !self.accounts.setup_required()?
            && self.accounts.settings()?.require_approval_for_datasets;
        for entry in dataset["entries"]
            .as_object()
            .context("Missing entries")?
            .values()
        {
            let recording_id = field(entry, "mcap_uuid")?;
            if !recordings.contains_key(recording_id) {
                let recording = self.recording_for_edit(id, recording_id)?;
                ensure!(
                    entry["raw_data"]["sha256"] == recording.item["sha256"]
                        && entry["raw_data"]["mcap"] == recording.item["path"],
                    "Dataset provenance does not match recording"
                );
                validate_annotations(&recording.index, &manifest, &recording.annotations, None)?;
                for (number, name) in recording.annotations["classes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                {
                    let identity = recording.annotations["class_uuids"][number]
                        .as_str()
                        .map(|uuid| json!(uuid))
                        .unwrap_or_else(|| json!({"name":name}));
                    if !identities.contains(&identity) {
                        identities.push(identity);
                        classes.push(name.clone());
                    }
                }
                let scopes = if dataset["format"] == "csv" {
                    self.approved_dataset_scopes(&recording, dataset_id)?
                } else {
                    self.approved_scopes(&recording)?
                };
                approved.insert(recording_id.to_owned(), scopes);
                workflows.insert(recording_id.to_owned(), recording_workflow(&recording));
                recordings.insert(recording_id.to_owned(), recording);
            }
        }
        let mut groups: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
        let mut csv_rows = Vec::new();
        let mut updated = 0;
        let mut copied = 0;
        let mut count = 0;
        let mut current = HashSet::new();
        let mut columns: Vec<String> = if dataset["format"] == "csv" {
            dataset["columns"]
                .as_array()
                .context("CSV needs columns")?
                .iter()
                .map(|column| field(column, "name").map(str::to_owned))
                .collect::<Result<_>>()?
        } else {
            vec![
                "image".into(),
                "annotation".into(),
                "mcap_uuid".into(),
                "channel_id".into(),
                "frame".into(),
                "time_ns".into(),
            ]
        };
        if columns.is_empty() {
            columns.push("value".into());
        }
        for (key, entry) in dataset["entries"].as_object().unwrap() {
            let recording_id = field(entry, "mcap_uuid")?;
            let channel = field(entry, "channel_id")?;
            let number = entry["frame"].as_u64().context("Missing frame")? as usize;
            ensure!(
                key == &format!("{recording_id}:{channel}:{number}"),
                "Dataset key does not match frame"
            );
            let recording = &recordings[recording_id];
            ensure!(
                entry["raw_data"]["sha256"] == recording.item["sha256"]
                    && entry["raw_data"]["mcap"] == recording.item["path"],
                "Dataset provenance does not match recording"
            );
            if require_approval
                && !approved[recording_id]
                    .iter()
                    .any(|scope| scope.contains(channel, number))
            {
                continue;
            }
            let frame = indexed_frame(&recording.index, channel, number)?;
            let mut sample = json!({"key":key,"mcap_uuid":recording_id,"channel_id":channel,"frame":number,"time_ns":frame["time_ns"]});
            let mut dependency = json!({"builder_version":1,"entry":entry,"format":format,"columns":dataset["columns"],"source_sha256":recording.item["sha256"]});
            // Only the relevant topic translation, plus its shared synchronization rule.
            let workflow = &workflows[recording_id];
            dependency["translation"] = workflow["translations"][channel].clone();
            dependency["synchronization"] = workflow["sync"].clone();
            if dataset["format"] == "csv" {
                sample["values"] = entry["values"].clone();
                ensure!(sample["values"].is_object(), "CSV entry needs values");
                csv_rows.push(
                    columns
                        .iter()
                        .map(|name| csv_cell(&sample["values"][name]))
                        .collect::<Vec<_>>()
                        .join(","),
                );
            } else {
                let row = &recording.annotations["annotations"][channel][number.to_string()];
                if row["reviewed"] != true
                    && (self.accounts.setup_required()? || !super::reviews::included(row))
                {
                    continue;
                }
                ensure!(
                    recording.index["topics"][channel]["kind"] == "image",
                    "Image datasets require image topics"
                );
                if format == "yolo" {
                    ensure!(row["labels"].as_array().is_none_or(Vec::is_empty), "YOLO detection cannot represent classification labels; generate CSV instead");
                }
                let width = frame["width"].as_f64().context("Missing width")?;
                let height = frame["height"].as_f64().context("Missing height")?;
                ensure!(width > 0.0 && height > 0.0, "Invalid image dimensions");
                let mut row = row.clone();
                let mut yolo = String::new();
                let empty = Vec::new();
                for bbox in row["boxes"].as_array_mut().unwrap_or(&mut Vec::new()) {
                    let source_class =
                        bbox["class_id"].as_u64().context("Invalid class ID")? as usize;
                    let identity = recording.annotations["class_uuids"][source_class]
                        .as_str()
                        .map(|uuid| json!(uuid))
                        .unwrap_or_else(
                            || json!({"name":recording.annotations["classes"][source_class]}),
                        );
                    let class = identities
                        .iter()
                        .position(|item| *item == identity)
                        .context("Unknown dataset class")?;
                    bbox["class_id"] = json!(class);
                    yolo.push_str(&format!(
                        "{} {:.10} {:.10} {:.10} {:.10}\n",
                        class,
                        (bbox["x"].as_f64().unwrap() + bbox["width"].as_f64().unwrap() / 2.0)
                            / width,
                        (bbox["y"].as_f64().unwrap() + bbox["height"].as_f64().unwrap() / 2.0)
                            / height,
                        bbox["width"].as_f64().unwrap() / width,
                        bbox["height"].as_f64().unwrap() / height
                    ));
                }
                let mapped_labels: Vec<_> = row["labels"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .map(|label| {
                        let source_class = label.as_u64().context("Invalid class ID")? as usize;
                        let identity = recording.annotations["class_uuids"][source_class]
                            .as_str()
                            .map(|uuid| json!(uuid))
                            .unwrap_or_else(
                                || json!({"name":recording.annotations["classes"][source_class]}),
                            );
                        identities
                            .iter()
                            .position(|item| *item == identity)
                            .context("Unknown dataset class")
                            .map(|class| json!(class))
                    })
                    .collect::<Result<_>>()?;
                row["labels"] = json!(mapped_labels);
                sample["annotation"] = row.clone();
                sample["width"] = frame["width"].clone();
                sample["height"] = frame["height"].clone();
                dependency["annotation"] = row;
                dependency["technique"] = recording.annotations["techniques"][channel].clone();
                let name = format!("{recording_id}_{channel}_{number:012}");
                let image_relative = format!("{base}/images/{name}.png");
                let source = contained_path(
                    &recording.cache,
                    &format!("frames/{}", field(frame, "image")?),
                )?;
                let source_stamp = fingerprint(&source)?;
                let image = contained_path(&folder, &image_relative)?;
                let image_stamp = fingerprint(&image).ok();
                if image_stamp.is_none()
                    || old[key]["source_image_stamp"] != source_stamp
                    || old[key]["image_stamp"] != json!(image_stamp)
                {
                    fs::create_dir_all(image.parent().unwrap())?;
                    let temporary = tempfile::NamedTempFile::new_in(image.parent().unwrap())?;
                    fs::copy(&source, temporary.path())?;
                    temporary.as_file().sync_all()?;
                    temporary.persist(&image).map_err(|error| error.error)?;
                    copied += 1;
                }
                sample["image"] = json!(image_relative);
                sample["source_image_stamp"] = source_stamp.clone();
                sample["image_stamp"] = fingerprint(&image)?;
                dependency["source_image_stamp"] = source_stamp;
                if format == "yolo" {
                    let label_relative = format!("{base}/labels/{name}.txt");
                    write_text_changed(
                        &contained_path(&folder, &label_relative)?,
                        yolo.as_bytes(),
                    )?;
                    sample["label"] = json!(label_relative);
                }
                if format == "csv" {
                    csv_rows.push(
                        [
                            csv_cell(&sample["image"]),
                            csv_cell(&sample["annotation"]),
                            csv_cell(&sample["mcap_uuid"]),
                            csv_cell(&sample["channel_id"]),
                            number.to_string(),
                            csv_cell(&sample["time_ns"]),
                        ]
                        .join(","),
                    );
                }
            }
            sample["dependency"] = json!(digest(&dependency)?);
            if old.get(key) != Some(&sample) {
                updated += 1;
            }
            groups
                .entry(format!(
                    "{recording_id}/{channel}/{:012}",
                    number as u64 / PAGE_SIZE
                ))
                .or_default()
                .insert(key.clone(), sample);
            current.insert(key.clone());
            count += 1;
        }
        let pages = write_pages(&folder, &format!("{base}/samples"), groups)?;
        if format == "csv" {
            let header = columns
                .iter()
                .map(|name| csv_cell(&json!(name)))
                .collect::<Vec<_>>()
                .join(",");
            let text = format!(
                "{header}\r\n{}",
                csv_rows
                    .iter()
                    .map(|row| format!("{row}\r\n"))
                    .collect::<String>()
            );
            write_text_changed(
                &contained_path(&folder, &format!("{base}/data.csv"))?,
                text.as_bytes(),
            )?;
        } else {
            fs::create_dir_all(contained_path(&folder, &format!("{base}/images"))?)?;
            fs::create_dir_all(contained_path(&folder, &format!("{base}/labels"))?)?;
            write_text_changed(
                &contained_path(&folder, &format!("{base}/data.yaml"))?,
                format!(
                    "path: .\ntrain: images\nnames: {}\n",
                    serde_json::to_string(&classes)?
                )
                .as_bytes(),
            )?;
        }
        let generated = json!({"format_version":1,"builder_version":1,"project_uuid":id,"dataset_uuid":dataset_id,"format":format,"sample_count":count,"sample_pages":pages,"classes":classes,"class_uuids":identities,"columns":columns});
        write_changed(&index_path, &generated)?;
        // Removed/unreviewed samples must also leave the standard YOLO directory.
        let mut removed = 0;
        for (key, sample) in &old {
            if !current.contains(key) {
                removed += 1;
                for field in ["image", "label"] {
                    if let Some(relative) = sample[field].as_str() {
                        let path = contained_path(&folder, relative)?;
                        ensure!(
                            path.starts_with(&output),
                            "Generated output escapes dataset"
                        );
                        if path.is_file() {
                            fs::remove_file(path)?;
                        }
                    }
                }
            }
        }
        Ok(
            json!({"dataset_uuid":dataset_id,"format":format,"path":base,"manifest":format!("{base}/manifest.json"),"sample_count":count,"updated_samples":updated,"copied_images":copied,"removed_samples":removed,"reused_samples":count-updated}),
        )
    }
}
