//! Queues, status counts and bounded CSV review samples.
use super::*;

impl Store {
    pub fn review_sample(
        &self,
        project: &str,
        task_id: &str,
        actor: &Principal,
        channel: &str,
        number: usize,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        let permissions = self.accounts.project_permissions(actor, &folder)?;
        ensure!(
            permissions.contains("project.view") && permissions.contains("mcap.view"),
            "Forbidden: mcap.view required"
        );
        let task = read_tasks(&folder, None)?
            .into_iter()
            .find(|t| t.id == task_id)
            .context("Task not found")?;
        ensure!(
            permissions.contains("review.view")
                || (permissions.contains("annotation.view") && task.annotator_id == actor.id),
            "Forbidden: task access required"
        );
        ensure!(
            task.scopes.iter().any(|s| s.contains(channel, number)),
            "Frame is outside the review task"
        );
        if task.current_revision > 0 {
            return self.review_revision_sample(
                project,
                task_id,
                task.current_revision,
                actor,
                channel,
                number,
            );
        }
        let dataset_id = task.dataset_id.context("Task has no CSV dataset")?;
        let (_, manifest) = self.manifest(project)?;
        let dataset = manifest["datasets"]
            .get(&dataset_id)
            .context("Dataset not found")?;
        let key = format!("{}:{channel}:{number}", task.recording_id);
        let entry = if let Some(pages) = dataset["entry_pages"].as_object() {
            let bucket = format!(
                "{}/{channel}/{:012}",
                task.recording_id,
                number as u64 / crate::storage::PAGE_SIZE
            );
            let page = pages.get(&bucket).context("Dataset sample not found")?;
            let selected = json!({bucket:page});
            crate::storage::read_pages(
                &folder,
                &format!("datasets/{dataset_id}/entries"),
                &selected,
            )?
            .get(&key)
            .context("Dataset sample not found")?
            .clone()
        } else {
            dataset["entries"]
                .get(&key)
                .context("Dataset sample not found")?
                .clone()
        };
        Ok(json!({"frame":number,"values":entry["values"],"time_ns":entry["time_ns"]}))
    }
    pub fn recording_review_counts(&self, project: &str, recording_id: &str) -> Result<Value> {
        if self.accounts.setup_required()? {
            let recording = self.recording_inner(project, recording_id)?;
            let mut counts = serde_json::Map::new();
            for channel in recording.index["topics"]
                .as_object()
                .context("Missing topics")?
                .keys()
            {
                counts.insert(
                    channel.clone(),
                    json!(self.eligible_rows(&recording, channel)?.len()),
                );
            }
            return Ok(json!(counts));
        }
        let data = self.approved_data(project)?;
        let mut latest = std::collections::BTreeSet::new();
        let source = self.recording_source(project, recording_id, false, false)?;
        let mut counts = source.index["topics"]
            .as_object()
            .context("Missing topics")?
            .keys()
            .map(|k| (k.clone(), json!(0)))
            .collect::<serde_json::Map<_, _>>();
        for source in data["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["recording_id"] == recording_id)
        {
            if !latest.insert(source["task_id"].as_str().unwrap_or("")) {
                continue;
            }
            for modality in source["modalities"].as_array().unwrap() {
                let channel = modality["channel_id"].as_str().unwrap_or("");
                let count = modality["annotated_samples"].as_u64().unwrap_or(0);
                let previous = counts.get(channel).and_then(Value::as_u64).unwrap_or(0);
                counts.insert(channel.into(), json!(previous + count));
            }
        }
        Ok(json!(counts))
    }
    pub fn review_tasks(&self, project: &str, actor: &Principal, queue: bool) -> Result<Value> {
        let folder = self.folder(project)?;
        let permissions = self.accounts.project_permissions(actor, &folder)?;
        let can_review = permissions.contains("review.view");
        ensure!(
            permissions.contains("project.view")
                && (can_review || permissions.contains("annotation.view")),
            "Forbidden: task access required"
        );
        ensure!(!queue || can_review, "Forbidden: review.view required");
        self.migrate_legacy_reviews(project)?;
        let tasks: Vec<_> = read_tasks(&folder, None)?
            .into_iter()
            .filter(|t| !queue || matches!(t.status, Status::Submitted | Status::UnderReview))
            .map(|mut task| {
                if !can_review
                    && !permissions.contains("project.manage")
                    && task.annotator_id != actor.id
                {
                    task.history.clear();
                }
                task
            })
            .collect();
        let mut values = vec![];
        for task in tasks {
            let revision = if task.current_revision > 0 {
                Some(read_revision(&folder, &task, task.current_revision)?)
            } else {
                None
            };
            let mut value = json!(task);
            if let Some(revision) = &revision {
                value["status"] = json!(revision.status);
            }
            value["latest_revision"] = json!(revision);
            value["previous_revision"] = previous_revision_summary(&folder, &task)?;
            values.push(value);
        }
        Ok(json!(values))
    }
}

pub(super) fn previous_revision_summary(folder: &Path, task: &Task) -> Result<Value> {
    if task.current_revision <= 1 {
        return Ok(Value::Null);
    }
    let previous = read_revision(folder, task, task.current_revision - 1)?;
    Ok(
        json!({"number":previous.number,"status":previous.status,"comment_count":previous.comments.len()}),
    )
}
