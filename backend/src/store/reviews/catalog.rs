//! Approved snapshot discovery. Cached summaries depend on immutable revisions,
//! never on the current working copy or a rescanned MCAP.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::store) struct ApprovedSource {
    pub project_id: String,
    pub folder: std::path::PathBuf,
    pub task: Task,
    pub revision: Revision,
    pub snapshot: Value,
}

impl ApprovedSource {
    pub fn reference(&self) -> Value {
        json!({"project_uuid":self.project_id,"task_id":self.task.id,"recording_id":self.task.recording_id,"revision":self.revision.number,"revision_id":self.revision.id,"snapshot_sha256":self.revision.snapshot.sha256,"source_sha256":self.snapshot["recording"]["sha256"],"source_file":self.snapshot["recording"]["source_file"],"annotator":self.revision.submitted_by_name,"reviewer":self.revision.reviewer_name,"approved_at":self.revision.reviewed_at,"scopes":self.revision.scopes})
    }
}

pub(in crate::store) fn modality(topic: &Value) -> (&'static str, &'static str) {
    let text = format!(
        "{} {} {}",
        topic["topic"].as_str().unwrap_or(""),
        topic["schema_name"].as_str().unwrap_or(""),
        topic["image_encoding"].as_str().unwrap_or("")
    )
    .to_ascii_lowercase();
    if topic["kind"] == "image" {
        if text.contains("depth") || text.contains("32fc1") || text.contains("16uc1") {
            ("depth", "Depth camera")
        } else {
            ("rgb", "RGB camera")
        }
    } else if text.contains("imu") || text.contains("inertial") {
        ("imu", "IMU")
    } else if text.contains("joint") || text.contains("motor") {
        ("joint_states", "Joint states")
    } else if text.contains("wrench") || text.contains("force") || text.contains("torque") {
        ("force_torque", "Force / torque")
    } else if text.contains("pose") || text.contains("odometry") || text.contains("end_effector") {
        ("pose", "End-effector pose")
    } else if text.contains("pointcloud") || text.contains("points") {
        ("pointcloud", "Point cloud")
    } else {
        ("structured", "Sensor messages")
    }
}

impl Store {
    pub(in crate::store) fn approved_sources(&self, project: &str) -> Result<Vec<ApprovedSource>> {
        let folder = self.folder(project)?;
        let mut sources = vec![];
        for task in read_tasks(&folder, None)? {
            for revision in revision_list(&folder, &task)?
                .into_iter()
                .filter(|r| r.status == Status::Approved && r.snapshot_available)
            {
                let snapshot = read_snapshot(&folder, &revision)?;
                ensure!(
                    snapshot["project_uuid"] == project,
                    "Approved snapshot project identity does not match"
                );
                sources.push(ApprovedSource {
                    project_id: project.into(),
                    folder: folder.clone(),
                    task: task.clone(),
                    revision,
                    snapshot,
                });
            }
        }
        sources.sort_by(|a, b| b.revision.reviewed_at.cmp(&a.revision.reviewed_at));
        Ok(sources)
    }
    pub(in crate::store) fn approved_source(
        &self,
        project: &str,
        task_id: &str,
        number: u64,
    ) -> Result<ApprovedSource> {
        let folder = self.folder(project)?;
        let task = read_tasks(&folder, None)?
            .into_iter()
            .find(|t| t.id == task_id)
            .context("Approved source task not found")?;
        let revision = read_revision(&folder, &task, number)?;
        ensure!(
            revision.status == Status::Approved,
            "Only an approved revision can be a production dataset input"
        );
        let snapshot = read_snapshot(&folder, &revision)?;
        ensure!(
            snapshot["project_uuid"] == project,
            "Approved source project identity does not match"
        );
        Ok(ApprovedSource {
            project_id: project.into(),
            folder,
            task,
            revision,
            snapshot,
        })
    }
    pub fn approved_data(&self, project: &str) -> Result<Value> {
        self.migrate_legacy_reviews(project)?;
        let (_, manifest) = self.manifest(project)?;
        let mut sources = vec![];
        let mut labels: BTreeMap<String, Value> = BTreeMap::new();
        let mut modalities: BTreeMap<String, Value> = BTreeMap::new();
        for source in self.approved_sources(project)? {
            // A deleted recording remains in audit history, but cannot be offered
            // as a buildable input until its exact source is restored.
            let item = &manifest["mcaps"][&source.task.recording_id];
            if item["sha256"] != source.snapshot["recording"]["sha256"] {
                continue;
            }
            let raw = contained_path(&source.folder, field(item, "path")?)?;
            if !raw.is_file() {
                continue;
            }
            let mut summary = self.approved_source_summary(&source)?;
            summary["project_name"] = manifest["name"].clone();
            for label in summary["labels"].as_array().into_iter().flatten() {
                let id = field(label, "id")?;
                let entry = labels.entry(id.into()).or_insert_with(
                    || json!({"id":id,"name":label["name"],"samples":0,"revisions":0,"aliases":[]}),
                );
                entry["samples"] = json!(
                    entry["samples"].as_u64().unwrap() + label["samples"].as_u64().unwrap_or(0)
                );
                entry["revisions"] = json!(entry["revisions"].as_u64().unwrap() + 1);
                if !entry["aliases"]
                    .as_array()
                    .unwrap()
                    .contains(&label["name"])
                {
                    entry["aliases"]
                        .as_array_mut()
                        .unwrap()
                        .push(label["name"].clone());
                }
            }
            for input in summary["modalities"].as_array().into_iter().flatten() {
                let id = field(input, "id")?;
                modalities
                    .entry(id.into())
                    .or_insert_with(|| json!({"id":id,"name":input["name"]}));
            }
            sources.push(summary);
        }
        Ok(
            json!({"project_uuid":project,"sources":sources,"labels":labels.into_values().collect::<Vec<_>>(),"modalities":modalities.into_values().collect::<Vec<_>>() }),
        )
    }
    pub(in crate::store) fn approved_source_summary(
        &self,
        source: &ApprovedSource,
    ) -> Result<Value> {
        let cache = contained_path(
            &source.folder,
            &format!(
                "indexes/approved/{}/{}.json",
                source.revision.id, source.revision.snapshot.sha256
            ),
        )?;
        if let Ok(value) = read_json(&cache) {
            if value["snapshot_sha256"] == source.revision.snapshot.sha256
                && value["catalog_version"] == 1
            {
                return Ok(value);
            }
        }
        let snapshot = &source.snapshot;
        let index = self.shared_json(&contained_path(
            &source.folder,
            field(&snapshot["index"], "path")?,
        )?)?;
        let annotations = snapshot_annotations(&source.folder, snapshot)?;
        let mut channel_counts: BTreeMap<String, BTreeMap<String, (String, u64)>> = BTreeMap::new();
        let mut channel_samples: BTreeMap<String, u64> = BTreeMap::new();
        let mut intervals = vec![];
        for scope in &source.revision.scopes {
            let frames = index["topics"][&scope.channel_id]["frames"]
                .as_array()
                .context("Revision topic frames not found")?;
            if let (Some(first), Some(last)) =
                (frames.get(scope.start_frame), frames.get(scope.end_frame))
            {
                intervals.push((
                    first["time_ns"].as_u64().unwrap_or(0),
                    last["time_ns"].as_u64().unwrap_or(0),
                ));
            }
            for (number, row) in annotations["annotations"][&scope.channel_id]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(n, r)| {
                    n.parse()
                        .is_ok_and(|n| scope.contains(&scope.channel_id, n))
                        && included(r)
                })
            {
                let _ = number;
                *channel_samples.entry(scope.channel_id.clone()).or_default() += 1;
                let mut ids = BTreeSet::new();
                for id in row["boxes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|b| &b["class_id"])
                    .chain(row["labels"].as_array().into_iter().flatten())
                {
                    if let Some(id) = id.as_u64() {
                        ids.insert(id as usize);
                    }
                }
                for class in ids {
                    let id = annotations["class_uuids"][class]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("legacy:{}:{class}", source.task.recording_id));
                    let name = annotations["classes"][class]
                        .as_str()
                        .unwrap_or("Unknown class")
                        .to_owned();
                    channel_counts
                        .entry(scope.channel_id.clone())
                        .or_default()
                        .entry(id)
                        .or_insert((name, 0))
                        .1 += 1;
                }
            }
        }
        for (id, saved) in snapshot["datasets"].as_object().into_iter().flatten() {
            let dataset = snapshot_dataset(&source.folder, snapshot, id)?;
            let channel = field(&saved["metadata"], "source_channel")?;
            for entry in dataset["entries"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(_, e)| e)
            {
                *channel_samples.entry(channel.into()).or_default() += 1;
                let mut seen = BTreeSet::new();
                for column in dataset["columns"].as_array().into_iter().flatten() {
                    let Some(list) = column["label_list"].as_str() else {
                        continue;
                    };
                    let value = &entry["values"][column["name"].as_str().unwrap_or("")];
                    for label in snapshot["label_lists"][list]["label_ids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        let Some(label) = label.as_str() else {
                            continue;
                        };
                        if value == &snapshot["labels"][label]["name"] && seen.insert(label) {
                            channel_counts
                                .entry(channel.into())
                                .or_default()
                                .entry(label.into())
                                .or_insert((
                                    snapshot["labels"][label]["name"]
                                        .as_str()
                                        .unwrap_or(label)
                                        .into(),
                                    0,
                                ))
                                .1 += 1;
                        }
                    }
                }
            }
        }
        let mut all_labels: BTreeMap<String, (String, u64)> = BTreeMap::new();
        let mut inputs = vec![];
        let mut types = BTreeSet::new();
        for (channel, topic) in index["topics"]
            .as_object()
            .context("Invalid revision index")?
        {
            let frames = topic["frames"].as_array().context("Invalid topic frames")?;
            let native = frames
                .iter()
                .filter(|f| {
                    f["time_ns"].as_u64().is_some_and(|time| {
                        intervals
                            .iter()
                            .any(|(start, end)| time >= *start && time <= *end)
                    })
                })
                .count();
            if native == 0 {
                continue;
            }
            let (kind, name) = modality(topic);
            let direct = channel_samples.get(channel).copied().unwrap_or(0);
            let labels = channel_counts
                .get(channel)
                .map(|counts| {
                    counts
                        .iter()
                        .map(|(id, (name, count))| {
                            all_labels.entry(id.clone()).or_insert((name.clone(), 0)).1 += count;
                            json!({"id":id,"name":name,"samples":count})
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if direct > 0 {
                types.insert(
                    annotations["techniques"][channel]
                        .as_str()
                        .unwrap_or("csv")
                        .to_owned(),
                );
            }
            inputs.push(json!({"id":kind,"name":name,"channel_id":channel,"topic":topic["topic"],"kind":topic["kind"],"native_samples":native,"annotated_samples":direct,"labels":labels,"associated":direct==0}));
        }
        let value = json!({"catalog_version":1,"project_uuid":source.project_id,"task_id":source.task.id,"revision_id":source.revision.id,"revision":source.revision.number,"recording_id":source.task.recording_id,"source_file":snapshot["recording"]["source_file"],"snapshot_sha256":source.revision.snapshot.sha256,"source_sha256":snapshot["recording"]["sha256"],"scopes":source.revision.scopes,"submitted_by_id":source.revision.submitted_by_id,"annotator":source.revision.submitted_by_name,"reviewer_id":source.revision.reviewer_id,"reviewer":source.revision.reviewer_name,"submitted_at":source.revision.submitted_at,"approved_at":source.revision.reviewed_at,"labels":all_labels.into_iter().map(|(id,(name,samples))|json!({"id":id,"name":name,"samples":samples})).collect::<Vec<_>>(),"modalities":inputs,"annotation_types":types,"sample_count":channel_samples.values().sum::<u64>(),"dataset_id":source.task.dataset_id});
        crate::storage::write_changed(&cache, &value)?;
        Ok(value)
    }
}
