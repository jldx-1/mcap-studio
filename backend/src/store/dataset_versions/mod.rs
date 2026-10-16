//! Production datasets are immutable recipes and sample memberships pinned to
//! approved annotation revisions. Materializations can be rebuilt separately.
mod assets;
mod generation;
mod selection;
mod synchronization;
use super::reviews::{modality, ApprovedSource};
use super::{
    canonical_uuid, contained_path, field, load_manifest, now, read_json, valid_name, Store,
};
use crate::storage::{digest, read_pages, write_changed, write_json, write_pages, PAGE_SIZE};
use anyhow::{ensure, Context, Result};
use selection::{select_samples, version_labels};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use uuid::Uuid;

fn version_path(dataset: &str, number: u64) -> Result<String> {
    canonical_uuid(dataset)?;
    ensure!(number > 0, "Invalid dataset version");
    Ok(format!(
        "datasets/{dataset}/versions/{number:012}/version.json"
    ))
}
fn read_version(folder: &Path, dataset: &str, number: u64) -> Result<Value> {
    let value = read_json(&contained_path(folder, &version_path(dataset, number)?)?)
        .context("Dataset version not found")?;
    ensure!(
        value["kind"] == "approved-dataset-version"
            && value["dataset_uuid"] == dataset
            && value["number"] == number,
        "Dataset version identity does not match"
    );
    Ok(value)
}

impl Store {
    pub fn create_dataset_version(
        &self,
        project: &str,
        dataset_id: Option<&str>,
        actor: &crate::accounts::Principal,
        request: &Value,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        self.accounts
            .require_project(actor, &folder, "datasets.generate")?;
        let requested = request["sources"]
            .as_array()
            .context("Select approved source revisions")?;
        ensure!(
            !requested.is_empty() && requested.len() <= 10000,
            "Select 1–10,000 source revisions"
        );
        let mut sources = vec![];
        let mut seen = BTreeSet::new();
        for reference in requested {
            let source_project = reference["project_uuid"].as_str().unwrap_or(project);
            let source_folder = self.folder(source_project)?;
            self.accounts
                .require_project(actor, &source_folder, "project.view")?;
            if source_project != project {
                self.accounts
                    .require_project(actor, &source_folder, "datasets.export")?;
            }
            let task = field(reference, "task_id")?;
            let number = reference["revision"]
                .as_u64()
                .context("Choose an approved revision number")?;
            ensure!(
                seen.insert((source_project.to_owned(), task.to_owned())),
                "Select only one approved revision of each annotation task in a dataset version"
            );
            self.migrate_legacy_reviews(source_project)?;
            let source = self.approved_source(source_project, task, number)?;
            if let Some(hash) = reference.get("snapshot_sha256") {
                ensure!(
                    *hash == source.revision.snapshot.sha256,
                    "Approved source snapshot does not match selection"
                );
            }
            sources.push(source);
        }
        sources.sort_by(|a, b| {
            a.project_id
                .cmp(&b.project_id)
                .then(a.task.id.cmp(&b.task.id))
        });
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = load_manifest(&folder)?;
        Self::editable(&manifest)?;
        let before = manifest.clone();
        let id = if let Some(id) = dataset_id {
            canonical_uuid(id)?;
            ensure!(manifest["datasets"].get(id).is_some(), "Dataset not found");
            ensure!(
                manifest["datasets"][id]["source_recording_uuid"].is_null(),
                "Create a separate training dataset from working CSV label tables"
            );
            id.to_owned()
        } else {
            Uuid::new_v4().to_string()
        };
        let name = if dataset_id.is_some() {
            field(&manifest["datasets"][&id], "name")?.to_owned()
        } else {
            valid_name(request)?
        };
        ensure!(
            !manifest["datasets"]
                .as_object()
                .context("Invalid dataset registry")?
                .iter()
                .any(|(other, d)| *other != id && d["name"] == name),
            "Dataset name already exists"
        );
        let number = manifest["datasets"][&id]["latest_version"]
            .as_u64()
            .unwrap_or(0)
            + 1;
        if let Some(expected) = request["base_version"].as_u64() {
            ensure!(dataset_id.is_some() && expected + 1 == number, "A newer dataset version was published. Reopen the dataset before adding recordings.");
        }
        let modalities: BTreeSet<String> = serde_json::from_value(request["modalities"].clone())?;
        ensure!(
            !modalities.is_empty()
                && modalities.iter().all(|m| [
                    "rgb",
                    "depth",
                    "imu",
                    "joint_states",
                    "pose",
                    "force_torque",
                    "pointcloud",
                    "structured"
                ]
                .contains(&m.as_str())),
            "Select supported input modalities"
        );
        let label_ids: BTreeSet<String> = if request["label_ids"].is_null() {
            BTreeSet::new()
        } else {
            serde_json::from_value(request["label_ids"].clone())?
        };
        let labels = version_labels(&sources, &label_ids)?;
        let splits = request
            .get("splits")
            .cloned()
            .unwrap_or(json!({"train":80,"val":10,"test":10,"seed":"0"}));
        let weights: Vec<f64> = ["train", "val", "test"]
            .iter()
            .map(|key| splits[key].as_f64().unwrap_or(-1.0))
            .collect();
        ensure!(
            weights.iter().all(|w| w.is_finite() && *w >= 0.0)
                && (weights.iter().sum::<f64>() - 100.0).abs() < 0.001,
            "Dataset split percentages must total 100"
        );
        let mut split_groups = serde_json::Map::new();
        for source in &sources {
            let manifest = self.cached_manifest(&source.folder)?;
            let item = &manifest["mcaps"][&source.task.recording_id];
            let group = item["session_id"]
                .as_str()
                .or(item["source_root_uuid"].as_str())
                .unwrap_or(&source.task.recording_id);
            split_groups.insert(
                format!("{}/{}", source.project_id, source.task.recording_id),
                json!(format!("{}/{group}", source.project_id)),
            );
        }
        let selection = json!({"modalities":modalities,"label_ids":label_ids,"include_negatives":request["include_negatives"].as_bool().unwrap_or(true),"primary_modality":request["primary_modality"],"synchronization":request["synchronization"],"sensor_window_ns":request["sensor_window_ns"],"splits":splits,"split_groups":split_groups});
        let base = format!("datasets/{id}/versions/{number:012}");
        ensure!(
            !contained_path(&folder, &format!("{base}/version.json"))?.is_file(),
            "Dataset version already exists"
        );
        let samples = select_samples(self, &folder, &base, &sources, &selection, &labels)?;
        ensure!(
            samples.count > 0,
            "No approved samples match these labels, modalities, and synchronization rules"
        );
        let references: Vec<_> = sources.iter().map(ApprovedSource::reference).collect();
        let default_format = if samples.images && !samples.classifications {
            "yolo"
        } else {
            "csv"
        };
        let version = json!({"kind":"approved-dataset-version","format_version":1,"dataset_uuid":id,"project_uuid":project,"number":number,"name":name,"created_at":now(),"created_by_id":actor.id,"created_by_name":actor.name,"source_revisions":references,"selection":selection,"labels":labels,"sample_pages":samples.pages,"sample_count":samples.count,"dropped_unsynchronized":samples.dropped,"default_format":default_format,"columns":samples.columns,"builder_version":2});
        write_json(
            &contained_path(&folder, &version_path(&id, number)?)?,
            &version,
        )?;
        let descriptor = json!({"number":number,"path":version_path(&id,number)?,"sha256":digest(&version)?,"sample_count":samples.count,"created_at":version["created_at"],"source_count":references.len()});
        let mut metadata = manifest["datasets"]
            .get(&id)
            .cloned()
            .unwrap_or_else(|| json!({"uuid":id,"name":name,"versions":[]}));
        metadata.as_object_mut().unwrap().remove("entries");
        if !metadata["versions"].is_array() {
            metadata["versions"] = json!([]);
        }
        metadata["versions"]
            .as_array_mut()
            .unwrap()
            .push(descriptor);
        metadata["kind"] = json!("approved-dataset");
        metadata["format"] = json!("versioned");
        metadata["latest_version"] = json!(number);
        metadata["default_format"] = json!(default_format);
        metadata["entry_pages"] = version["sample_pages"].clone();
        metadata["entry_count"] = json!(samples.count);
        metadata["modalities"] = selection["modalities"].clone();
        metadata["multimodal"] = json!(modalities.len() > 1);
        metadata["sources"]=json!(references.iter().map(|r|json!({"mcap_uuid":r["recording_id"],"project_uuid":r["project_uuid"],"source_file":r["source_file"],"channel_id":"approved","topic":"Approved revision","modality":"multimodal","entry_count":samples.count})).collect::<Vec<_>>());
        // Keep references from earlier versions visible to source-deletion checks.
        let mut retained: BTreeSet<String> = before["datasets"][&id]["all_source_recordings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_str().map(str::to_owned))
            .collect();
        retained.extend(
            references
                .iter()
                .filter_map(|r| r["recording_id"].as_str().map(str::to_owned)),
        );
        metadata["all_source_recordings"] = json!(retained);
        manifest["datasets"][&id] = metadata;
        crate::storage::save_manifest_delta(&folder, &manifest, &before)?;
        crate::accounts::append_audit(
            &contained_path(&folder, "audit/datasets.jsonl")?,
            &json!({"at":now(),"actor_id":actor.id,"actor_name":actor.name,"action":"dataset.version_created","dataset_id":id,"version":number,"source_revisions":references}),
        )?;
        Ok(
            json!({"dataset":manifest["datasets"][&id],"version":version,"project":self.detail(&folder,&manifest)}),
        )
    }
    pub fn dataset_versions(&self, project: &str, dataset: &str) -> Result<Value> {
        let (_, manifest) = self.manifest(project)?;
        let data = manifest["datasets"]
            .get(dataset)
            .context("Dataset not found")?;
        Ok(data["versions"].clone())
    }
    pub fn dataset_version(&self, project: &str, dataset: &str, number: u64) -> Result<Value> {
        let folder = self.folder(project)?;
        let value = read_version(&folder, dataset, number)?;
        self.verify_dataset_version(&folder, dataset, number, &value)?;
        Ok(value)
    }
    fn verify_dataset_version(
        &self,
        folder: &Path,
        dataset: &str,
        number: u64,
        value: &Value,
    ) -> Result<()> {
        let manifest = self.cached_manifest(folder)?;
        let descriptor = manifest["datasets"][dataset]["versions"]
            .as_array()
            .context("Dataset has no versions")?
            .iter()
            .find(|v| v["number"] == number)
            .context("Dataset version not found")?;
        ensure!(
            descriptor["path"] == version_path(dataset, number)?
                && descriptor["sha256"] == digest(value)?,
            "Dataset version hash does not match immutable definition"
        );
        Ok(())
    }
    pub fn dataset_version_samples(
        &self,
        project: &str,
        dataset: &str,
        number: u64,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        let version = self.dataset_version(project, dataset, number)?;
        let mut skip = offset;
        let mut rows = vec![];
        let limit = limit.clamp(1, 200);
        for (bucket, page) in version["sample_pages"]
            .as_object()
            .context("Invalid sample pages")?
        {
            let count = page["count"].as_u64().context("Invalid page count")? as usize;
            if skip >= count {
                skip -= count;
                continue;
            }
            let values = read_pages(
                &folder,
                &format!("datasets/{dataset}/versions/{number:012}/samples"),
                &json!({bucket:page}),
            )?;
            for sample in values.values().skip(skip).take(limit - rows.len()) {
                rows.push(sample.clone());
            }
            skip = 0;
            if rows.len() >= limit {
                break;
            }
        }
        Ok(json!({"total":version["sample_count"],"offset":offset,"samples":rows}))
    }
}
