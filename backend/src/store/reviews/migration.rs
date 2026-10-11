//! Older task histories are preserved. Recover the latest snapshot only when
//! its original content digest proves the current working copy is identical.
use super::*;

impl Store {
    pub(in crate::store) fn migrate_legacy_reviews(&self, project: &str) -> Result<()> {
        let folder = self.folder(project)?;
        if !read_tasks(&folder, None)?.iter().any(|t| {
            t.current_revision == 0 && t.history.iter().any(|e| e["action"] == "task.submit")
        }) {
            return Ok(());
        }
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        for mut task in read_tasks(&folder, None)?
            .into_iter()
            .filter(|t| t.current_revision == 0)
        {
            let mut revisions: Vec<Revision> = vec![];
            for event in &task.history {
                let action = event["action"].as_str().unwrap_or("");
                if action == "task.submit" {
                    revisions.push(Revision {
                        id: Uuid::new_v4().to_string(),
                        task_id: task.id.clone(),
                        recording_id: task.recording_id.clone(),
                        number: revisions.len() as u64 + 1,
                        scopes: task.scopes.clone(),
                        submitted_by_id: event["actor_id"]
                            .as_str()
                            .unwrap_or(&task.annotator_id)
                            .into(),
                        submitted_by_name: event["actor_name"]
                            .as_str()
                            .unwrap_or(&task.annotator_name)
                            .into(),
                        submitted_at: event["at"].as_str().unwrap_or(&task.updated_at).into(),
                        reviewed_at: None,
                        review_started_at: None,
                        self_review: false,
                        reviewer_id: None,
                        reviewer_name: None,
                        status: Status::UnderReview,
                        snapshot: SnapshotRef {
                            path: String::new(),
                            sha256: event["details"]["digest"].as_str().unwrap_or("").into(),
                        },
                        snapshot_available: false,
                        frame_count: task.frame_count,
                        annotated_count: event["details"]["annotated_count"]
                            .as_u64()
                            .unwrap_or(task.annotated_count as u64)
                            as usize,
                        comments: vec![],
                        version: 0,
                    });
                    continue;
                }
                let Some(revision) = revisions.last_mut() else {
                    continue;
                };
                if action == "task.claim" {
                    revision.reviewer_id = event["actor_id"].as_str().map(str::to_owned);
                    revision.reviewer_name = event["actor_name"].as_str().map(str::to_owned);
                    revision.review_started_at = event["at"].as_str().map(str::to_owned);
                }
                if ["task.comment", "task.reply", "task.request_changes"].contains(&action) {
                    if let Some(message) = event["details"]["message"].as_str() {
                        revision.comments.push(ReviewComment {
                            id: Uuid::new_v4().to_string(),
                            actor_id: event["actor_id"].as_str().unwrap_or("").into(),
                            actor_name: event["actor_name"]
                                .as_str()
                                .unwrap_or("Unknown user")
                                .into(),
                            at: event["at"].as_str().unwrap_or("").into(),
                            message: message.into(),
                            scope: serde_json::from_value(event["details"]["scope"].clone()).ok(),
                        });
                    }
                }
                if action == "task.request_changes" {
                    revision.status = Status::ChangesRequested;
                    revision.reviewed_at = event["at"].as_str().map(str::to_owned);
                }
                if action == "task.approve" {
                    revision.status = Status::Approved;
                    revision.reviewed_at = event["at"].as_str().map(str::to_owned);
                    revision.reviewer_id = event["actor_id"].as_str().map(str::to_owned);
                    revision.reviewer_name = event["actor_name"].as_str().map(str::to_owned);
                }
                if action == "task.withdraw" {
                    revision.status = Status::Withdrawn;
                    revision.reviewed_at = event["at"].as_str().map(str::to_owned);
                }
            }
            if revisions.is_empty() {
                continue;
            }
            let last = revisions.last_mut().unwrap();
            if let Ok(recording) = self.recording_for_edit(project, &task.recording_id) {
                if annotation_digest(&recording, &task.scopes, task.dataset_id.as_deref())
                    .is_ok_and(|hash| hash == last.snapshot.sha256)
                {
                    last.snapshot = publish_snapshot(&recording, &task)?;
                    last.snapshot_available = true;
                }
            }
            task.status = last.status.clone();
            task.current_revision = revisions.len() as u64;
            task.working_dirty = true;
            for revision in &revisions {
                save_revision(&folder, &task, revision)?;
            }
            task.version += 1;
            save_task(&folder, &task)?;
        }
        Ok(())
    }
}
