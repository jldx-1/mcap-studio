//! Task creation and reviewer eligibility.
use super::*;

impl Store {
    pub fn create_review_task(
        &self,
        project: &str,
        actor: &Principal,
        request: &Value,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(project)?;
        let _lock = Self::lock_project(&folder)?;
        self.accounts
            .require_project(actor, &folder, "annotation.submit")?;
        let recording_id = field(request, "recording_id")?;
        let recording = self.recording_for_edit(project, recording_id)?;
        let scopes: Vec<Scope> = serde_json::from_value(request["scopes"].clone())?;
        ensure!(
            !scopes.is_empty() && scopes.len() <= 64,
            "Provide 1–64 review scopes"
        );
        for (i, scope) in scopes.iter().enumerate() {
            validate_scope(&recording, scope)?;
            ensure!(
                !scopes[..i].iter().any(|s| s.overlaps(scope)),
                "Review scopes overlap"
            );
        }
        let existing = read_tasks(&folder, Some(recording_id))?;
        ensure!(
            !existing
                .iter()
                .filter(|t| t.dataset_id.as_deref() == request["dataset_id"].as_str())
                .any(|t| t
                    .scopes
                    .iter()
                    .any(|s| scopes.iter().any(|scope| s.overlaps(scope)))),
            "A task already exists for this range; use its existing submission workflow"
        );
        let title = request["title"]
            .as_str()
            .unwrap_or("Annotation task")
            .trim();
        ensure!(
            !title.is_empty() && title.len() <= 240,
            "Invalid task title"
        );
        let dataset_id = request["dataset_id"].as_str().map(str::to_owned);
        if let Some(id) = &dataset_id {
            canonical_uuid(id)?;
            let dataset = recording.project["datasets"]
                .get(id)
                .context("Dataset not found")?;
            ensure!(
                dataset["format"] == "csv"
                    && dataset["source_recording_uuid"] == recording_id
                    && scopes
                        .iter()
                        .all(|s| dataset["source_channel"] == s.channel_id),
                "Task dataset must belong to the selected recording and topic"
            );
        }
        let count = count_annotations(&recording, &scopes, dataset_id.as_deref())?;
        let mut task = Task {
            id: Uuid::new_v4().to_string(),
            recording_id: recording_id.into(),
            dataset_id,
            title: title.into(),
            frame_count: scopes.iter().map(|s| s.end_frame - s.start_frame + 1).sum(),
            scopes,
            annotator_id: actor.id.clone(),
            annotator_name: actor.name.clone(),
            reviewer_id: None,
            reviewer_name: None,
            status: if count > 0 {
                Status::InProgress
            } else {
                Status::Unlabeled
            },
            current_revision: 0,
            working_dirty: true,
            source_sha256: field(&recording.item, "sha256")?.into(),
            submitted_digest: None,
            annotated_count: count,
            approved_sections: vec![],
            history: vec![],
            version: 0,
            created_at: now(),
            updated_at: now(),
        };
        if let Some(reviewer) = request["reviewer_id"].as_str().filter(|s| !s.is_empty()) {
            self.accounts
                .require_project(actor, &folder, "review.assign")?;
            assign_reviewer(self, &folder, &mut task, reviewer, false)?;
        }
        let details = json!({"scopes":task.scopes});
        event(&mut task, actor, "task.created", details);
        save_task(&folder, &task)?;
        audit(&folder, &task)?;
        Ok(json!(task))
    }
}

pub(super) fn assign_reviewer(
    store: &Store,
    folder: &Path,
    task: &mut Task,
    reviewer: &str,
    explicit_self_review: bool,
) -> Result<()> {
    let directory = store.accounts.project_directory(folder)?;
    let user = directory
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == reviewer)
        .context("Reviewer not found in project")?;
    ensure!(
        user["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "review.view"),
        "Assigned user needs review.view"
    );
    ensure!(
        explicit_self_review
            || !store.accounts.settings()?.independent_reviewer
            || read_revision(folder, task, task.current_revision)
                .map(|r| r.submitted_by_id)
                .unwrap_or_else(|_| task.annotator_id.clone())
                != reviewer,
        "Forbidden: an independent reviewer is required"
    );
    task.reviewer_id = Some(reviewer.into());
    task.reviewer_name = user["name"].as_str().map(str::to_owned);
    Ok(())
}
