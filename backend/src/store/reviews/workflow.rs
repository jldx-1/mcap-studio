//! A submission publishes a new immutable snapshot. Review metadata and comments
//! belong to its revision, independently of subsequent working-copy saves.
use super::*;

impl Store {
    pub fn review_task_action(
        &self,
        project: &str,
        task_id: &str,
        actor: &Principal,
        request: &Value,
    ) -> Result<Value> {
        canonical_uuid(task_id)?;
        self.migrate_legacy_reviews(project)?;
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(project)?;
        let _lock = Self::lock_project(&folder)?;
        self.accounts
            .require_project(actor, &folder, "project.view")?;
        let mut task = read_tasks(&folder, None)?
            .into_iter()
            .find(|t| t.id == task_id)
            .context("Task not found")?;
        let action = field(request, "action")?;
        // Claiming an available review for oneself needs review access, while
        // assigning others or taking their assignments needs review.assign.
        let self_assignment = action == "assign"
            && request["reviewer_id"].as_str() == Some(actor.id.as_str())
            && task.reviewer_id.as_ref().is_none_or(|id| id == &actor.id);
        let permission=match action {
            "submit"|"withdraw"=>"annotation.submit", "reopen"=>"annotation.edit",
            "claim"=>"review.view", "assign" if self_assignment=>"review.view",
            "assign"|"assign_annotator"=>"review.assign",
            "approve"=>"review.approve", "request_changes"=>"review.reject",
            "comment"=>"review.comment", "reply"=>"annotation.submit",
            "approve_section"=>anyhow::bail!("Approve the complete revision, or leave section feedback and request a new revision"),
            _=>anyhow::bail!("Unknown review action"),
        };
        self.accounts.require_project(actor, &folder, permission)?;
        if matches!(action, "submit" | "withdraw" | "reopen" | "reply") {
            ensure!(
                task.annotator_id == actor.id,
                "Forbidden: only the assigned annotator can submit this working copy"
            );
        }
        let number = request["revision_number"]
            .as_u64()
            .unwrap_or(task.current_revision);
        let mut revision = if task.current_revision > 0 {
            Some(read_revision(&folder, &task, number)?)
        } else {
            None
        };
        if let Some(expected) = request["expected_revision_version"].as_u64() {
            ensure!(
                revision.as_ref().is_some_and(|r| r.version == expected),
                "Revision changed; refresh before acting"
            );
        } else {
            ensure!(
                request["expected_version"].as_u64() == Some(task.version),
                "Task changed; refresh before acting"
            );
        }
        let mut details = json!({"revision":number});
        match action {
            "submit" => {
                if let Some(scopes) = request.get("scopes") {
                    let scopes: Vec<Scope> = serde_json::from_value(scopes.clone())?;
                    ensure!(
                        !scopes.is_empty() && scopes.len() <= 64,
                        "Provide 1–64 review scopes"
                    );
                    for (i, s) in scopes.iter().enumerate() {
                        ensure!(
                            !scopes[..i].iter().any(|other| other.overlaps(s)),
                            "Review scopes overlap"
                        );
                    }
                    let other = read_tasks(&folder, Some(&task.recording_id))?;
                    ensure!(
                        !other
                            .iter()
                            .filter(|t| t.id != task.id && t.dataset_id == task.dataset_id)
                            .any(|t| t
                                .scopes
                                .iter()
                                .any(|old| scopes.iter().any(|new| old.overlaps(new)))),
                        "Range belongs to another annotation task"
                    );
                    task.scopes = scopes;
                }
                let recording = self.recording_for_edit(project, &task.recording_id)?;
                for scope in &task.scopes {
                    validate_scope(&recording, scope)?;
                    ensure!(
                        task.dataset_id.is_some()
                            || recording.annotations["techniques"][&scope.channel_id]
                                .as_str()
                                .is_some_and(|s| s != "undefined"),
                        "Choose a labeling technique before submitting"
                    );
                }
                super::super::validate_annotations(
                    &recording.index,
                    &recording.project,
                    &recording.annotations,
                    None,
                )?;
                task.annotated_count =
                    count_annotations(&recording, &task.scopes, task.dataset_id.as_deref())?;
                ensure!(task.annotated_count>0,"Save at least one annotated or explicitly included empty frame before submission");
                // A newer submission closes an unfinished review. Its comments and
                // snapshot remain attached to that older revision.
                if task.current_revision > 0 {
                    let mut previous = read_revision(&folder, &task, task.current_revision)?;
                    if matches!(previous.status, Status::Submitted | Status::UnderReview) {
                        previous.status = Status::Superseded;
                        previous.reviewed_at = Some(now());
                        previous.version += 1;
                        save_revision(&folder, &task, &previous)?;
                    }
                }
                let snapshot = publish_snapshot(&recording, &task)?;
                let retain_self_review = revision.as_ref().is_some_and(|previous| {
                    previous.self_review
                        && previous.submitted_by_id == actor.id
                        && task.reviewer_id.as_deref() == Some(actor.id.as_str())
                });
                task.current_revision += 1;
                task.frame_count = task
                    .scopes
                    .iter()
                    .map(|s| s.end_frame - s.start_frame + 1)
                    .sum();
                task.source_sha256 = field(&recording.item, "sha256")?.into();
                task.submitted_digest = Some(snapshot.sha256.clone());
                task.status = Status::Submitted;
                task.working_dirty = false;
                task.approved_sections.clear();
                // Deleted or reassigned reviewers do not strand a new submission.
                if let Some(id) = task.reviewer_id.clone() {
                    if assign_reviewer(self, &folder, &mut task, &id, retain_self_review).is_err() {
                        task.reviewer_id = None;
                        task.reviewer_name = None;
                    }
                }
                let submitted = Revision {
                    id: Uuid::new_v4().to_string(),
                    task_id: task.id.clone(),
                    recording_id: task.recording_id.clone(),
                    number: task.current_revision,
                    scopes: task.scopes.clone(),
                    submitted_by_id: actor.id.clone(),
                    submitted_by_name: actor.name.clone(),
                    submitted_at: now(),
                    reviewed_at: None,
                    review_started_at: None,
                    self_review: retain_self_review && task.reviewer_id.is_some(),
                    reviewer_id: task.reviewer_id.clone(),
                    reviewer_name: task.reviewer_name.clone(),
                    status: Status::Submitted,
                    snapshot,
                    snapshot_available: true,
                    frame_count: task.frame_count,
                    annotated_count: task.annotated_count,
                    comments: vec![],
                    version: 0,
                };
                save_revision(&folder, &task, &submitted)?;
                details = json!({"revision":submitted.number,"snapshot":submitted.snapshot,"annotated_count":submitted.annotated_count});
                revision = Some(submitted);
            }
            "assign_annotator" => {
                let id = field(request, "annotator_id")?;
                let users = self.accounts.project_directory(&folder)?;
                let user = users
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|u| u["id"] == id)
                    .context("Annotator not found in project")?;
                ensure!(
                    user["permissions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|p| p == "annotation.submit"),
                    "Assigned annotator needs annotation.submit"
                );
                task.annotator_id = id.into();
                task.annotator_name = field(user, "name")?.into();
                details =
                    json!({"annotator_id":task.annotator_id,"revision":task.current_revision});
            }
            "reopen" => {
                // Kept as an API compatibility alias. It starts working-copy edits;
                // it never revokes or modifies an approved revision.
                task.working_dirty = true;
            }
            _ => {
                let reviewed = revision
                    .as_mut()
                    .context("Submit a revision before reviewing")?;
                ensure!(
                    number == task.current_revision,
                    "Previous revisions are read-only"
                );
                ensure!(
                    matches!(reviewed.status, Status::Submitted | Status::UnderReview),
                    "This revision is closed and read-only"
                );
                if matches!(action, "claim" | "approve" | "request_changes" | "comment") {
                    self.accounts
                        .require_project(actor, &folder, "review.view")?;
                    ensure!(
                        !self.accounts.settings()?.independent_reviewer
                            || reviewed.submitted_by_id != actor.id
                            || (reviewed.self_review
                                && reviewed.reviewer_id.as_deref() == Some(actor.id.as_str())),
                        "Forbidden: an independent reviewer is required"
                    );
                    ensure!(
                        reviewed
                            .reviewer_id
                            .as_ref()
                            .is_none_or(|id| id == &actor.id),
                        "Forbidden: revision is assigned to another reviewer"
                    );
                }
                match action {
                    "claim" => {
                        ensure!(
                            reviewed.review_started_at.is_none(),
                            "This revision is already claimed"
                        );
                        assign_reviewer(self, &folder, &mut task, &actor.id, reviewed.self_review)?;
                        reviewed.reviewer_id = Some(actor.id.clone());
                        reviewed.reviewer_name = Some(actor.name.clone());
                        reviewed.review_started_at = Some(now());
                        reviewed.status = Status::UnderReview;
                    }
                    "assign" => {
                        let id = request["reviewer_id"].as_str().unwrap_or("");
                        let explicit_self_review = id == actor.id && id == reviewed.submitted_by_id;
                        if id.is_empty() {
                            task.reviewer_id = None;
                            task.reviewer_name = None;
                        } else {
                            assign_reviewer(self, &folder, &mut task, id, explicit_self_review)?;
                        }
                        reviewed.reviewer_id = task.reviewer_id.clone();
                        reviewed.reviewer_name = task.reviewer_name.clone();
                        reviewed.review_started_at = None;
                        reviewed.self_review = explicit_self_review;
                        reviewed.status = Status::Submitted;
                        details = json!({"revision":number,"reviewer_id":reviewed.reviewer_id,"self_review":reviewed.self_review});
                    }
                    "withdraw" => {
                        reviewed.status = Status::Withdrawn;
                        reviewed.reviewed_at = Some(now());
                        task.working_dirty = true;
                    }
                    "approve" | "request_changes" => {
                        ensure!(
                            reviewed.review_started_at.is_some()
                                && reviewed.reviewer_id.as_deref() == Some(&actor.id),
                            "Claim the revision before making a review decision"
                        );
                        // Validate the published snapshot, not the mutable working copy.
                        read_snapshot(&folder, reviewed)?;
                        if action == "request_changes" {
                            let comments = feedback_comments(reviewed, actor, request)?;
                            ensure!(
                                !comments.is_empty()
                                    || reviewed.comments.iter().any(|c| c.actor_id == actor.id),
                                "Provide review feedback before requesting another revision"
                            );
                            reviewed.comments.extend(comments);
                            reviewed.status = Status::ChangesRequested;
                            task.working_dirty = true;
                        } else {
                            let source =
                                self.recording_source(project, &task.recording_id, false, false)?;
                            let frozen = read_snapshot(&folder, reviewed)?;
                            ensure!(
                                source.item["sha256"] == frozen["recording"]["sha256"],
                                "Revision source SHA-256 no longer matches the recording"
                            );
                            reviewed.reviewer_name = Some(actor.name.clone());
                            reviewed.status = Status::Approved;
                        }
                        reviewed.reviewed_at = Some(now());
                    }
                    "comment" | "reply" => {
                        if action == "comment" {
                            ensure!(
                                reviewed.review_started_at.is_some()
                                    && reviewed.reviewer_id.as_deref() == Some(&actor.id),
                                "Claim the revision before publishing review feedback"
                            );
                        }
                        let comments = feedback_comments(reviewed, actor, request)?;
                        ensure!(!comments.is_empty(), "Provide review feedback");
                        reviewed.comments.extend(comments);
                    }
                    _ => unreachable!(),
                }
                reviewed.version += 1;
                save_revision(&folder, &task, reviewed)?;
                task.status = reviewed.status.clone();
                task.reviewer_id = reviewed.reviewer_id.clone();
                task.reviewer_name = reviewed.reviewer_name.clone();
                details["revision"] = json!(number);
                details["comments"] = json!(reviewed.comments.len());
            }
        }
        event(&mut task, actor, &format!("task.{action}"), details);
        save_task(&folder, &task)?;
        audit(&folder, &task)?;
        let mut result = json!(task);
        result["latest_revision"] = json!(revision);
        result["previous_revision"] = previous_revision_summary(&folder, &task)?;
        Ok(result)
    }
}

fn feedback_comments(
    revision: &Revision,
    actor: &Principal,
    request: &Value,
) -> Result<Vec<ReviewComment>> {
    let supplied: Vec<Value> = if let Some(comments) = request["comments"].as_array() {
        comments.clone()
    } else if request["message"]
        .as_str()
        .is_some_and(|m| !m.trim().is_empty())
    {
        vec![request.clone()]
    } else {
        vec![]
    };
    ensure!(supplied.len() <= 100, "Submit at most 100 comments at once");
    supplied
        .into_iter()
        .map(|value| {
            let message = value["message"].as_str().unwrap_or("").trim();
            ensure!(
                !message.is_empty() && message.len() <= 8000,
                "Provide feedback of 1–8000 bytes"
            );
            let scope = if value["scope"].is_null() {
                None
            } else {
                let scope: Scope = serde_json::from_value(value["scope"].clone())?;
                ensure!(
                    scope.start_frame <= scope.end_frame
                        && revision
                            .scopes
                            .iter()
                            .any(|s| s.channel_id == scope.channel_id
                                && s.start_frame <= scope.start_frame
                                && s.end_frame >= scope.end_frame),
                    "Feedback range must be inside this revision"
                );
                Some(scope)
            };
            Ok(ReviewComment {
                id: Uuid::new_v4().to_string(),
                actor_id: actor.id.clone(),
                actor_name: actor.name.clone(),
                at: now(),
                message: message.into(),
                scope,
            })
        })
        .collect()
}
