use super::*;

impl Store {
    pub fn invalidate_label_reviews(&self, project: &str, actor: &Principal) -> Result<()> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(project)?;
        let _lock = Self::lock_project(&folder)?;
        self.accounts
            .require_project(actor, &folder, "labels.manage")?;
        for mut task in read_tasks(&folder, None)?.into_iter().filter(|t| {
            matches!(
                t.status,
                Status::Submitted | Status::UnderReview | Status::Approved
            )
        }) {
            task.working_dirty = true;
            event(
                &mut task,
                actor,
                "labels.changed",
                json!({"message":"Working-copy label definitions changed. Submitted revisions retain their original definitions."}),
            );
            save_task(&folder, &task)?;
            audit(&folder, &task)?;
        }
        Ok(())
    }

    pub(in crate::store) fn check_dataset_task_edit(
        &self,
        folder: &Path,
        dataset_id: &str,
        actor: &Principal,
    ) -> Result<()> {
        let permissions = self.accounts.project_permissions(actor, folder)?;
        for task in read_tasks(folder, None)?
            .into_iter()
            .filter(|t| t.dataset_id.as_deref() == Some(dataset_id))
        {
            ensure!(
                task.annotator_id == actor.id || permissions.contains("project.manage"),
                "Forbidden: task belongs to another annotator"
            );
        }
        Ok(())
    }
    /// Called under the same lock as annotation persistence, preventing edits
    /// racing with a review decision. Returns affected tasks for the audit write.
    pub(in crate::store) fn authorize_annotation_changes(
        &self,
        recording: &Recording,
        actor: &Principal,
        changes: &Value,
        metadata: &Value,
    ) -> Result<Vec<Task>> {
        let permissions = self
            .accounts
            .project_permissions(actor, &recording.folder)?;
        ensure!(
            permissions.contains("project.view"),
            "Forbidden: project membership required"
        );
        ensure!(
            ["annotation.create", "annotation.edit", "annotation.delete"]
                .iter()
                .any(|p| permissions.contains(*p)),
            "Forbidden: annotation mutation permission required"
        );
        let mut affected = Vec::new();
        let mut impacts: Vec<(String, Option<usize>)> = vec![];
        for change in changes.as_array().context("Provide annotation changes")? {
            let channel = field(change, "channel_id")?;
            let number = change["frame"].as_u64().context("Invalid frame")? as usize;
            let previous = &recording.annotations["annotations"][channel][number.to_string()];
            let row = &change["annotation"];
            if same_json(previous, row) {
                continue;
            }
            let permission = if row.is_null() {
                "annotation.delete"
            } else if previous.is_null() {
                "annotation.create"
            } else {
                "annotation.edit"
            };
            ensure!(
                permissions.contains(permission),
                "Forbidden: {permission} required"
            );
            impacts.push((channel.into(), Some(number)));
        }
        if let Some(metadata) = metadata.as_object() {
            for (key, value) in metadata {
                if same_json(value, &recording.annotations[key]) {
                    continue;
                }
                ensure!(
                    [
                        "classes",
                        "class_uuids",
                        "techniques",
                        "topic_labels",
                        "topic_label_lists",
                        "tracks",
                        "settings"
                    ]
                    .contains(&key.as_str()),
                    "Cannot write workflow or reserved annotation metadata"
                );
                if matches!(key.as_str(), "classes" | "class_uuids") {
                    ensure!(
                        permissions.contains("labels.manage"),
                        "Forbidden: labels.manage required to change class definitions"
                    );
                }
                ensure!(
                    permissions.contains("annotation.edit"),
                    "Forbidden: annotation.edit required for annotation configuration"
                );
                if key == "settings" {
                    continue;
                }
                if matches!(
                    key.as_str(),
                    "tracks" | "techniques" | "topic_labels" | "topic_label_lists"
                ) {
                    let keys: std::collections::BTreeSet<_> = value
                        .as_object()
                        .into_iter()
                        .flatten()
                        .map(|(k, _)| k)
                        .chain(
                            recording.annotations[key]
                                .as_object()
                                .into_iter()
                                .flatten()
                                .map(|(k, _)| k),
                        )
                        .collect();
                    for channel in keys {
                        if !same_json(&value[channel], &recording.annotations[key][channel]) {
                            impacts.push((channel.clone(), None));
                        }
                    }
                } else {
                    impacts.push((String::new(), None));
                }
            }
        }
        if impacts.is_empty() {
            return Ok(affected);
        }
        for task in read_tasks(&recording.folder, Some(field(&recording.item, "uuid")?))?
            .into_iter()
            .filter(|t| t.dataset_id.is_none())
        {
            if task.scopes.iter().any(|s| {
                impacts.iter().any(|(c, n)| {
                    c.is_empty() || (s.channel_id == *c && n.is_none_or(|n| s.contains(c, n)))
                })
            }) {
                ensure!(
                    task.annotator_id == actor.id || permissions.contains("project.manage"),
                    "Forbidden: this annotation task belongs to another annotator"
                );

                affected.push(task);
            }
        }
        Ok(affected)
    }
    pub(in crate::store) fn annotation_change_audit(
        &self,
        recording: &Recording,
        actor: &Principal,
        mut tasks: Vec<Task>,
        changes: &Value,
    ) -> Result<()> {
        for task in &mut tasks {
            if task.current_revision == 0 && task.status == Status::Unlabeled {
                task.status = Status::InProgress;
            }
            task.working_dirty = true;
            event(
                task,
                actor,
                "annotations.updated",
                json!({"changed_frames":changes.as_array().map(Vec::len).unwrap_or(0)}),
            );
            save_task(&recording.folder, task)?;
            audit(&recording.folder, task)?;
        }
        append_audit(
            &contained_path(&recording.folder, "audit/annotations.jsonl")?,
            &json!({"at":now(),"actor_id":actor.id,"actor_name":actor.name,"recording_id":recording.item["uuid"],"action":"annotations.saved","changed_frames":changes.as_array().map(Vec::len).unwrap_or(0)}),
        )
    }
}
