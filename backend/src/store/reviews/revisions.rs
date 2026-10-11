//! Review metadata belongs to one submitted snapshot; a closed review is read-only.
use super::*;

fn revision_path(task: &Task, number: u64) -> Result<String> {
    canonical_uuid(&task.id)?;
    canonical_uuid(&task.recording_id)?;
    ensure!(number > 0, "Invalid revision number");
    Ok(format!(
        "reviews/{}/{}/revisions/{number:012}.json",
        task.recording_id, task.id
    ))
}
pub(super) fn read_revision(folder: &Path, task: &Task, number: u64) -> Result<Revision> {
    let mut revision: Revision = serde_json::from_value(
        read_json(&contained_path(folder, &revision_path(task, number)?)?)
            .context("Revision not found")?,
    )?;
    ensure!(
        revision.task_id == task.id
            && revision.recording_id == task.recording_id
            && revision.number == number,
        "Revision identity does not match"
    );
    // Older releases called waiting submissions "under_review". Interpret that
    // metadata without rewriting snapshots or changing a historical decision.
    if revision.status == Status::UnderReview && revision.review_started_at.is_none() {
        revision.status = Status::Submitted;
    }
    Ok(revision)
}
pub(super) fn save_revision(folder: &Path, task: &Task, revision: &Revision) -> Result<()> {
    write_json(
        &contained_path(folder, &revision_path(task, revision.number)?)?,
        &serde_json::to_value(revision)?,
    )
}
pub(super) fn revision_list(folder: &Path, task: &Task) -> Result<Vec<Revision>> {
    (1..=task.current_revision)
        .rev()
        .map(|n| read_revision(folder, task, n))
        .collect()
}

impl Store {
    pub fn review_revisions(
        &self,
        project: &str,
        task_id: &str,
        actor: &Principal,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        let task = self.visible_review_task(&folder, task_id, actor)?;
        Ok(json!(revision_list(&folder, &task)?))
    }
    fn visible_review_task(&self, folder: &Path, id: &str, actor: &Principal) -> Result<Task> {
        let permissions = self.accounts.project_permissions(actor, folder)?;
        ensure!(
            permissions.contains("project.view")
                && (permissions.contains("annotation.view") || permissions.contains("review.view")),
            "Forbidden: revision access required"
        );
        read_tasks(folder, None)?
            .into_iter()
            .find(|t| t.id == id)
            .context("Task not found")
    }
    pub(in crate::store) fn revision_recording(
        &self,
        project: &str,
        task: &Task,
        revision: &Revision,
    ) -> Result<Recording> {
        let folder = self.folder(project)?;
        let snapshot = read_snapshot(&folder, revision)?;
        ensure!(
            snapshot["project_uuid"] == project,
            "Revision project identity does not match"
        );
        let source = self.recording_source(project, &task.recording_id, false, false)?;
        ensure!(
            source.item["sha256"] == snapshot["recording"]["sha256"],
            "Revision source SHA-256 does not match recording"
        );
        let index_path = contained_path(&folder, field(&snapshot["index"], "path")?)?;
        let index = self.shared_json(&index_path)?;
        ensure!(
            digest(&index)? == snapshot["index"]["sha256"],
            "Revision index contents do not match the pinned snapshot"
        );
        ensure!(
            index["sha256"] == source.item["sha256"],
            "Revision index SHA-256 does not match recording"
        );
        let mut project = (*source.project).clone();
        project["labels"] = snapshot["labels"].clone();
        project["label_lists"] = snapshot["label_lists"].clone();
        Ok(Recording {
            project,
            item: snapshot["recording"].clone(),
            folder,
            cache: source.cache,
            index,
            annotations: snapshot_annotations(&source.folder, &snapshot)?,
        })
    }
    pub fn review_revision_recording(
        &self,
        project: &str,
        task_id: &str,
        number: u64,
        actor: &Principal,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        self.accounts.require_project(actor, &folder, "mcap.view")?;
        let task = self.visible_review_task(&folder, task_id, actor)?;
        let revision = read_revision(&folder, &task, number)?;
        let recording = self.revision_recording(project, &task, &revision)?;
        let mut value = super::super::recordings::recording_detail(&recording);
        let snapshot = read_snapshot(&folder, &revision)?;
        value["revision_datasets"]=json!(snapshot["datasets"].as_object().into_iter().flatten().map(|(id,value)|json!({"uuid":id,"name":value["metadata"]["name"],"source_channel":value["metadata"]["source_channel"]})).collect::<Vec<_>>());
        value["revision"] = json!(revision);
        Ok(value)
    }
    pub fn review_revision_sample(
        &self,
        project: &str,
        task_id: &str,
        number: u64,
        actor: &Principal,
        channel: &str,
        frame: usize,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        self.accounts.require_project(actor, &folder, "mcap.view")?;
        let task = self.visible_review_task(&folder, task_id, actor)?;
        let revision = read_revision(&folder, &task, number)?;
        ensure!(
            revision.scopes.iter().any(|s| s.contains(channel, frame)),
            "Frame is outside this revision"
        );
        let snapshot = read_snapshot(&folder, &revision)?;
        let id = task
            .dataset_id
            .as_deref()
            .or_else(|| {
                snapshot["datasets"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .find(|(_, d)| d["metadata"]["source_channel"] == channel)
                    .map(|(id, _)| id.as_str())
            })
            .context("Revision has no CSV dataset for this topic")?;
        let saved = &snapshot["datasets"][id];
        let metadata = &saved["metadata"];
        let bucket = format!(
            "{}/{channel}/{:012}",
            task.recording_id,
            frame as u64 / crate::storage::PAGE_SIZE
        );
        let page = metadata["entry_pages"]
            .get(&bucket)
            .context("Revision sample not found")?;
        let rows = crate::storage::read_pages(
            &folder,
            field(saved, "pages_base")?,
            &json!({bucket:page}),
        )?;
        let key = format!("{}:{channel}:{frame}", task.recording_id);
        let entry = rows.get(&key).context("Revision sample not found")?;
        Ok(
            json!({"frame":frame,"revision":number,"values":entry["values"],"time_ns":entry["time_ns"]}),
        )
    }
    pub fn review_revision_diff(
        &self,
        project: &str,
        task_id: &str,
        number: u64,
        base: u64,
        actor: &Principal,
        page: (usize, usize),
    ) -> Result<Value> {
        let (offset, limit) = page;
        let folder = self.folder(project)?;
        let task = self.visible_review_task(&folder, task_id, actor)?;
        let next = read_revision(&folder, &task, number)?;
        let previous = read_revision(&folder, &task, base)?;
        let a = read_snapshot(&folder, &previous)?;
        let b = read_snapshot(&folder, &next)?;
        let old = snapshot_annotations(&folder, &a)?;
        let new = snapshot_annotations(&folder, &b)?;
        let channels: std::collections::BTreeSet<_> = old["annotations"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k)
            .chain(
                new["annotations"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(k, _)| k),
            )
            .collect();
        let mut changes = vec![];
        for channel in channels {
            let frames: std::collections::BTreeSet<_> = old["annotations"][channel]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, _)| k)
                .chain(
                    new["annotations"][channel]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .map(|(k, _)| k),
                )
                .collect();
            for frame in frames {
                let before = &old["annotations"][channel][frame];
                let after = &new["annotations"][channel][frame];
                if !same_json(before, after) {
                    changes.push(json!({"channel_id":channel,"frame":frame.parse::<u64>()?,"kind":if before.is_null(){"added"}else if after.is_null(){"deleted"}else{"modified"},"before":before,"after":after}));
                }
            }
        }
        changes.sort_by(|a, b| {
            a["channel_id"]
                .as_str()
                .cmp(&b["channel_id"].as_str())
                .then(a["frame"].as_u64().cmp(&b["frame"].as_u64()))
        });
        Ok(
            json!({"base":base,"revision":number,"total":changes.len(),"changes":changes.into_iter().skip(offset).take(limit.clamp(1,500)).collect::<Vec<_>>(),"labels_changed":a["labels"]!=b["labels"],"ranges_changed":a["scopes"]!=b["scopes"],"csv_changed":a["datasets"]!=b["datasets"]}),
        )
    }
}
