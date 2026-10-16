//! Dataset membership, metadata, inspection and numeric analysis.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self},
    path::Path,
};
use uuid::Uuid;

use super::{
    canonical_uuid, contained_path, copy_tree, decoded_record, field, flatten_message,
    indexed_frame, load_manifest, read_json, selected_annotation_channels, stringify_timestamps,
    validate_annotations, Store,
};

impl Store {
    pub fn dataset_csv(&self, id: &str, dataset_id: &str) -> Result<Vec<u8>> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let (folder, manifest) = self.manifest(id)?;
        let dataset = manifest["datasets"]
            .get(dataset_id)
            .context("Dataset not found")?;
        ensure!(dataset["format"] == "csv", "Dataset is not CSV");
        if !self.accounts.setup_required()?
            && self.accounts.settings()?.require_approval_for_datasets
        {
            return self.approved_csv_data(id, dataset_id);
        }
        Ok(fs::read(contained_path(
            &folder,
            field(dataset, "csv_path")?,
        )?)?)
    }

    pub(super) fn dataset_metadata(
        &self,
        folder: &Path,
        project: &Value,
        dataset: &Value,
    ) -> Value {
        if dataset["kind"] == "imported-yolo" {
            return json!({"sources":[],"modalities":["image"],"multimodal":false});
        }
        if dataset["kind"] == "approved-dataset" {
            return serde_json::json!({"sources":dataset["sources"],"modalities":dataset["modalities"],"multimodal":dataset["multimodal"]});
        }
        if let Some(sources) = dataset["sources"]
            .as_array()
            .filter(|_| dataset.get("entries").is_none())
        {
            let mut sources = sources.clone();
            let mut modalities = std::collections::BTreeSet::new();
            for source in &mut sources {
                source["source_file"] = project["mcaps"]
                    [source["mcap_uuid"].as_str().unwrap_or_default()]["source_file"]
                    .clone();
                modalities.insert(source["modality"].as_str().unwrap_or("unknown").to_owned());
            }
            return json!({"sources":sources,"modalities":modalities,"multimodal":modalities.len() > 1});
        }
        let mut groups: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
        for entry in dataset["entries"]
            .as_object()
            .into_iter()
            .flat_map(|entries| entries.values())
        {
            let recording = entry["mcap_uuid"].as_str().unwrap_or_default();
            let channel = entry["channel_id"].as_str().unwrap_or_default();
            let group = groups
                .entry((recording.to_owned(), channel.to_owned()))
                .or_default();
            group.0 += 1;
            group.1 = entry["topic"].as_str().unwrap_or(channel).to_owned();
        }
        let mut indexes = HashMap::new();
        let mut modalities = std::collections::BTreeSet::new();
        let sources: Vec<_> = groups.into_iter().map(|((recording, channel), (count, topic))| {
            let item = &project["mcaps"][&recording];
            let index = indexes.entry(recording.clone()).or_insert_with(|| {
                let path = contained_path(folder, item["path"].as_str()?).ok()?;
                let original = path.parent()?;
                let cached = format!("data/cache/{}/{recording}/frames/index.json", project["uuid"].as_str()?);
                let path = contained_path(original, "frames/index.json").ok().filter(|path| path.is_file())
                    .or_else(|| contained_path(&self.workspace, &cached).ok())?;
                read_json(&path).ok().filter(|index| index["sha256"] == item["sha256"])
            });
            let kind = index.as_ref().and_then(|index| index["topics"][&channel]["kind"].as_str());
            let modality = match kind { Some("image") => "image", Some(_) => "structured", None if dataset["format"] == "csv" => "structured", None => "unknown" };
            modalities.insert(modality.to_owned());
            json!({"mcap_uuid":recording,"channel_id":channel,"source_file":item["source_file"],"topic":topic,"modality":modality,"entry_count":count})
        }).collect();
        json!({"sources":sources,"modalities":modalities,"multimodal":modalities.iter().filter(|kind| kind.as_str() != "unknown").count() > 1})
    }

    pub fn delete_dataset(&self, id: &str, dataset_id: &str) -> Result<Value> {
        self.delete_dataset_inner(id, dataset_id, None)
    }
    pub fn delete_dataset_as(
        &self,
        id: &str,
        dataset_id: &str,
        actor: &crate::accounts::Principal,
    ) -> Result<Value> {
        self.delete_dataset_inner(id, dataset_id, Some(actor))
    }
    fn delete_dataset_inner(
        &self,
        id: &str,
        dataset_id: &str,
        actor: Option<&crate::accounts::Principal>,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        canonical_uuid(dataset_id)?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        if let Some(actor) = actor {
            self.accounts
                .require_project(actor, &folder, "datasets.generate")?;
            self.check_dataset_task_edit(&folder, dataset_id, actor)?;
        }
        let mut manifest = load_manifest(&folder)?;
        Self::editable(&manifest)?;
        ensure!(
            manifest["datasets"].get(dataset_id).is_some(),
            "Dataset not found"
        );
        let directory = contained_path(&folder, &format!("datasets/{dataset_id}"))?;
        ensure!(
            directory == folder.join(format!("datasets/{dataset_id}")),
            "Dataset storage path does not match its own folder"
        );
        let retired = contained_path(&folder, &format!("datasets/.deleted-{}", Uuid::new_v4()))?;
        let has_files = directory.is_dir();
        if has_files {
            fs::rename(&directory, &retired)?;
        }
        manifest["datasets"]
            .as_object_mut()
            .context("Invalid dataset registry")?
            .remove(dataset_id);
        if let Err(error) = crate::storage::save_manifest(&folder, &manifest) {
            if has_files {
                let _ = fs::rename(&retired, &directory);
            }
            return Err(error);
        }
        if has_files {
            fs::remove_dir_all(retired)?;
        }
        Ok(self.detail(&folder, &manifest))
    }

    pub fn dataset_features(&self, id: &str, dataset_id: &str) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        self.dataset_features_inner(id, dataset_id)
    }

    pub(super) fn dataset_features_inner(&self, id: &str, dataset_id: &str) -> Result<Value> {
        let (folder, mut manifest) = self.manifest(id)?;
        if let Some(dataset) = manifest["datasets"].get_mut(dataset_id) {
            crate::storage::hydrate_dataset(&folder, dataset)?;
        }
        let dataset = manifest["datasets"]
            .get(dataset_id)
            .context("Dataset not found")?;
        let entries: Vec<_> = dataset["entries"]
            .as_object()
            .context("Invalid dataset entries")?
            .iter()
            .collect();
        ensure!(
            entries.len() >= 2,
            "PCA needs a dataset with at least two entries"
        );
        let sample_count = entries.len().min(5000);
        let mut recordings = HashMap::new();
        let mut rows = Vec::new();
        let mut fields = std::collections::BTreeSet::new();
        let mut errors = Vec::new();
        for position in 0..sample_count {
            let (key, entry) = entries[position * (entries.len() - 1) / (sample_count - 1)];
            let result = (|| -> Result<Value> {
                let recording_id = field(entry, "mcap_uuid")?;
                let channel = field(entry, "channel_id")?;
                let number = entry["frame"].as_u64().context("Entry needs a frame")? as usize;
                let values = if dataset["format"] == "csv" {
                    entry["values"].clone()
                } else {
                    if !recordings.contains_key(recording_id) {
                        recordings.insert(
                            recording_id.to_owned(),
                            self.recording_inner(id, recording_id)?,
                        );
                    }
                    let recording = &recordings[recording_id];
                    let topic = &recording.index["topics"][channel];
                    if topic["kind"] == "image" {
                        let frame = indexed_frame(&recording.index, channel, number)?;
                        let path = contained_path(
                            &recording.cache,
                            &format!("frames/{}", field(frame, "image")?),
                        )?;
                        let image = image::open(path)?.thumbnail(32, 32).to_rgb8();
                        let mut means = [0.0; 3];
                        let mut squares = [0.0; 3];
                        let pixels = (image.width() * image.height()) as f64;
                        ensure!(pixels > 0.0, "Image has no pixels");
                        for pixel in image.pixels() {
                            for index in 0..3 {
                                let value = pixel[index] as f64 / 255.0;
                                means[index] += value / pixels;
                                squares[index] += value * value / pixels;
                            }
                        }
                        let row =
                            &recording.annotations["annotations"][channel][number.to_string()];
                        let boxes = row["boxes"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                        let width = frame["width"].as_f64().unwrap_or(1.0);
                        let height = frame["height"].as_f64().unwrap_or(1.0);
                        let mut values = json!({"box_count":boxes.len(),"mean_box_area":if boxes.is_empty() {0.0} else { boxes.iter().map(|bbox| bbox["width"].as_f64().unwrap_or(0.0) * bbox["height"].as_f64().unwrap_or(0.0) / (width * height)).sum::<f64>() / boxes.len() as f64 }});
                        for (index, name) in ["red", "green", "blue"].iter().enumerate() {
                            values[format!("mean_{name}")] = json!(means[index]);
                            values[format!("std_{name}")] = json!((squares[index]
                                - means[index] * means[index])
                                .max(0.0)
                                .sqrt());
                        }
                        for (class_id, name) in recording.annotations["classes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .enumerate()
                        {
                            let count = boxes
                                .iter()
                                .filter(|bbox| bbox["class_id"] == class_id)
                                .count()
                                + row["labels"].as_array().map_or(0, |labels| {
                                    labels.iter().filter(|label| **label == class_id).count()
                                });
                            values[format!("class/{}", name.as_str().unwrap_or("unknown"))] =
                                json!(count);
                        }
                        values
                    } else {
                        decoded_record(recording, channel, number)?
                    }
                };
                let mut flattened = Map::new();
                flatten_message(&values, "", &mut flattened);
                let numeric: Map<_, _> = flattened
                    .into_iter()
                    .filter_map(|(name, value)| {
                        let number = if let Some(value) = value.as_bool() {
                            Some(if value { 1.0 } else { 0.0 })
                        } else {
                            value
                                .as_f64()
                                .or_else(|| value.as_str()?.parse::<f64>().ok())
                        }?;
                        (number.is_finite() && number.abs() <= 9_007_199_254_740_991.0)
                            .then(|| (name, json!(number)))
                    })
                    .collect();
                fields.extend(numeric.keys().cloned());
                Ok(
                    json!({"key":key,"mcap_uuid":recording_id,"channel_id":channel,"frame":number,"topic":entry["topic"],"values":numeric}),
                )
            })();
            match result {
                Ok(row) => rows.push(row),
                Err(error) => {
                    if errors.len() < 10 {
                        errors.push(format!("{key}: {error}"));
                    }
                }
            }
        }
        Ok(
            json!({"fields":fields,"rows":rows,"total_entries":entries.len(),"sampled_entries":sample_count,"unreadable_entries":sample_count - rows.len(),"errors":errors,"description":if dataset["format"] == "csv" {"Numeric and boolean dataset columns"} else {"Images: RGB color statistics, box counts, relative box area, and class counts. Structured messages: numeric fields."}}),
        )
    }

    pub fn dataset_pca(&self, id: &str, dataset_id: &str, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let features = self.dataset_features_inner(id, dataset_id)?;
        let selected = request["features"]
            .as_array()
            .context("Select numeric features")?;
        ensure!(
            (2..=32).contains(&selected.len()),
            "Select between 2 and 32 numeric features"
        );
        let mut seen = HashSet::new();
        let names: Vec<_> = selected
            .iter()
            .map(|feature| -> Result<&str> {
                let name = feature.as_str().context("Feature names must be strings")?;
                ensure!(
                    seen.insert(name) && features["fields"].as_array().unwrap().contains(feature),
                    "Select unique available features"
                );
                Ok(name)
            })
            .collect::<Result<_>>()?;
        let mut matrix = Vec::new();
        let mut included = Vec::new();
        let source = request["source"]
            .as_str()
            .filter(|source| !source.is_empty());
        let mut candidates = 0;
        for row in features["rows"].as_array().unwrap() {
            if source.is_some_and(|source| {
                source
                    != format!(
                        "{}:{}",
                        row["mcap_uuid"].as_str().unwrap_or_default(),
                        row["channel_id"].as_str().unwrap_or_default()
                    )
            }) {
                continue;
            }
            candidates += 1;
            let values: Option<Vec<f64>> = names
                .iter()
                .map(|name| row["values"][*name].as_f64())
                .collect();
            if let Some(values) = values {
                matrix.push(values);
                included.push(row);
            }
        }
        let mut result =
            crate::analysis::pca(&matrix, request["standardize"].as_bool().unwrap_or(true))?;
        let scores = result.as_object_mut().unwrap().remove("scores").unwrap();
        let points: Vec<_> = included.iter().zip(scores.as_array().unwrap()).map(|(row, score)| json!({"key":row["key"],"mcap_uuid":row["mcap_uuid"],"channel_id":row["channel_id"],"frame":row["frame"],"topic":row["topic"],"x":score[0],"y":score[1]})).collect();
        result["points"] = json!(points);
        result["features"] = json!(names);
        result["total_entries"] = features["total_entries"].clone();
        result["sampled_entries"] = features["sampled_entries"].clone();
        result["excluded_entries"] = json!(
            if source.is_some() {
                candidates
            } else {
                features["sampled_entries"].as_u64().unwrap() as usize
            } - included.len()
        );
        result["candidate_entries"] = json!(candidates);
        result["unreadable_entries"] = features["unreadable_entries"].clone();
        result["description"] = features["description"].clone();
        Ok(result)
    }

    pub fn dataset(&self, id: &str, dataset_id: &str) -> Result<Value> {
        let (folder, manifest) = self.manifest(id)?;
        let mut dataset = manifest["datasets"]
            .get(dataset_id)
            .cloned()
            .context("Dataset not found")?;
        if dataset["kind"] == "approved-dataset" {
            let number = dataset["latest_version"]
                .as_u64()
                .context("Dataset has no published version")?;
            dataset["current_version"] = self.dataset_version(id, dataset_id, number)?;
            return Ok(dataset);
        }
        crate::storage::hydrate_dataset(&folder, &mut dataset)?;
        let metadata = self.dataset_metadata(&folder, &manifest, &dataset);
        dataset
            .as_object_mut()
            .unwrap()
            .extend(metadata.as_object().unwrap().clone());
        stringify_timestamps(&mut dataset);
        Ok(dataset)
    }

    pub fn add_dataset_entries(
        &self,
        id: &str,
        dataset_id: &str,
        request: &Value,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let recording_id = field(request, "recording_uuid")?;
        let recording = self.recording_for_edit(id, recording_id)?;
        Self::editable(&recording.project)?;
        validate_annotations(
            &recording.index,
            &recording.project,
            &recording.annotations,
            None,
        )?;
        let mut manifest = load_manifest(&folder)?;
        ensure!(
            manifest["datasets"].get(dataset_id).is_some(),
            "Dataset not found"
        );
        crate::storage::hydrate_dataset(&folder, &mut manifest["datasets"][dataset_id])?;
        ensure!(
            manifest["datasets"][dataset_id]["format"] != "csv",
            "Edit CSV datasets through the message column workflow"
        );
        let channels = selected_annotation_channels(&recording, request)?;
        let mut entries = Map::new();
        let relative_dir = Path::new(field(&recording.item, "path")?)
            .parent()
            .unwrap()
            .to_string_lossy();
        // Materialize a managed read cache once, only when the user adds dataset membership.
        let original_cache = contained_path(&folder, &relative_dir)?;
        if original_cache != recording.cache {
            copy_tree(
                &recording.cache.join("frames"),
                &contained_path(&original_cache, "frames")?,
            )?;
        }
        for channel in channels {
            let topic = &recording.index["topics"][&channel];
            if topic["kind"] != "image" {
                ensure!(
                    request["channel_id"].is_null(),
                    "Message topics use the CSV column workflow"
                );
                continue;
            }
            let rows = self.eligible_rows(&recording, &channel)?;
            for (number, _) in rows {
                let frame = indexed_frame(&recording.index, &channel, number)?;
                let mut raw = json!({"mcap":recording.item["path"],"sha256":recording.item["sha256"],"frame_index":format!("{relative_dir}/frames/index.json")});
                if topic["kind"] == "image" {
                    raw["image"] =
                        json!(format!("{relative_dir}/frames/{}", field(frame, "image")?));
                } else {
                    raw["records"] = json!(format!(
                        "{relative_dir}/frames/channel_{channel}/records.jsonl"
                    ));
                    raw["offset"] = frame["offset"].clone();
                }
                contained_path(
                    &folder,
                    raw["image"]
                        .as_str()
                        .or(raw["records"].as_str())
                        .context("Missing raw data")?,
                )?
                .metadata()
                .context("Raw frame data is missing")?;
                let annotation_ref = if manifest["format_version"] == 3 {
                    crate::storage::annotation_path(recording_id)
                } else {
                    format!("{relative_dir}/annotations.json")
                };
                entries.insert(format!("{recording_id}:{channel}:{number}"),json!({"mcap_uuid":recording_id,"channel_id":channel,"topic":topic["topic"],"frame":number,"time_ns":frame["time_ns"],"raw_data":raw,"labeled_data":{"path":annotation_ref,"channel_id":channel,"frame":number}}));
            }
        }
        ensure!(
            !entries.is_empty(),
            "Approve at least one annotated or included empty frame before adding data"
        );
        let annotation_path = Path::new(field(&recording.item, "path")?)
            .parent()
            .unwrap()
            .join("annotations.json");
        if manifest["format_version"] == 3 {
            crate::storage::save_annotations(&folder, recording_id, &recording.annotations)?;
        } else {
            crate::storage::write_changed(
                &contained_path(&folder, &annotation_path.to_string_lossy())?,
                &recording.annotations,
            )?;
        }
        manifest["datasets"][dataset_id]["entries"]
            .as_object_mut()
            .context("Invalid dataset")?
            .extend(entries);
        crate::storage::save_manifest(&folder, &manifest)?;
        Ok(self.detail(&folder, &manifest))
    }
}
