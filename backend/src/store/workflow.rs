//! Persistent preparation recipes, dataset drafts and background job metadata.
use super::{
    canonical_uuid, contained_path, field, load_manifest, now, read_json, valid_name, write_json,
    Store,
};
use crate::accounts::Principal;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::PathBuf};
use uuid::Uuid;

impl Store {
    fn workflow_path(&self, project: &str, collection: &str, id: &str) -> Result<PathBuf> {
        canonical_uuid(id)?;
        contained_path(
            &self.folder(project)?,
            &format!("workflow/{collection}/{id}.json"),
        )
    }

    pub fn workflow_items(&self, project: &str, collection: &str) -> Result<Value> {
        ensure!(
            matches!(collection, "drafts" | "recipes" | "jobs"),
            "Unknown workflow collection"
        );
        let path = contained_path(&self.folder(project)?, &format!("workflow/{collection}"))?;
        let mut items = vec![];
        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                if entry.file_type()?.is_file()
                    && entry.path().extension().is_some_and(|s| s == "json")
                {
                    items.push(read_json(&entry.path())?);
                }
            }
        }
        items.sort_by(|a, b| b["updated_at"].as_str().cmp(&a["updated_at"].as_str()));
        Ok(json!(items))
    }

    pub fn save_workflow_item(
        &self,
        project: &str,
        collection: &str,
        request: &Value,
        actor: &Principal,
    ) -> Result<Value> {
        ensure!(
            matches!(collection, "drafts" | "recipes"),
            "Unknown workflow collection"
        );
        let folder = self.folder(project)?;
        let permission = if collection == "drafts" {
            "datasets.generate"
        } else {
            "project.manage"
        };
        self.accounts.require_project(actor, &folder, permission)?;
        let name = valid_name(request)?;
        let id = request["uuid"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let path = self.workflow_path(project, collection, &id)?;
        if collection == "drafts" {
            let recordings = request["recordings"]
                .as_array()
                .context("Choose recordings for the draft")?;
            ensure!(
                recordings.len() <= 10000,
                "Select at most 10,000 recordings"
            );
            let mut seen = BTreeSet::new();
            for reference in recordings {
                let source_project = field(reference, "project_uuid")?;
                let recording = field(reference, "recording_id")?;
                canonical_uuid(recording)?;
                ensure!(
                    seen.insert((source_project, recording)),
                    "Duplicate draft recording"
                );
                let source_folder = self.folder(source_project)?;
                self.accounts
                    .require_project(actor, &source_folder, "mcap.view")?;
                if source_project != project {
                    self.accounts
                        .require_project(actor, &source_folder, "datasets.export")?;
                }
                ensure!(
                    self.cached_manifest(&source_folder)?["mcaps"]
                        .get(recording)
                        .is_some(),
                    "Draft recording no longer exists"
                );
            }
            if let Some(target) = request["target_dataset"].as_str() {
                ensure!(
                    self.cached_manifest(&folder)?["datasets"][target]["kind"]
                        == "approved-dataset",
                    "Choose a versioned training dataset"
                );
            }
        } else {
            let rules = request["rules"]
                .as_array()
                .context("Recipe needs topic rules")?;
            ensure!(
                !rules.is_empty() && rules.len() <= 10000,
                "Recipe needs 1–10,000 topic rules"
            );
            for rule in rules {
                ensure!(
                    !field(rule, "topic")?.is_empty(),
                    "Recipe topic is required"
                );
                ensure!(
                    rule["keep"].is_boolean(),
                    "Specify whether each topic is kept"
                );
                if let Some(script) = rule["script"].as_str() {
                    ensure!(
                        script.len() <= 256 * 1024,
                        "Translation script is too large"
                    );
                }
            }
        }
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        Self::editable(&load_manifest(&folder)?)?;
        let old = if path.exists() {
            Some(read_json(&path)?)
        } else {
            None
        };
        let revision = old
            .as_ref()
            .and_then(|v| v["revision"].as_u64())
            .unwrap_or(0);
        ensure!(
            request["revision"].as_u64().unwrap_or(0) == revision,
            "This draft or recipe changed elsewhere. Reload before saving."
        );
        let mut item = request.clone();
        item["uuid"] = json!(id);
        item["name"] = json!(name);
        item["revision"] = json!(revision + 1);
        item["updated_at"] = json!(now());
        item["updated_by"] = json!(actor.name);
        item["project_uuid"] = json!(project);
        item.as_object_mut().unwrap().remove("sha256");
        item["sha256"] = json!(crate::storage::digest(&item)?);
        if collection == "recipes" {
            write_json(
                &contained_path(
                    &folder,
                    &format!("workflow/recipe-history/{id}/{:012}.json", revision + 1),
                )?,
                &item,
            )?;
        }
        write_json(&path, &item)?;
        Ok(item)
    }

    pub fn delete_workflow_item(
        &self,
        project: &str,
        collection: &str,
        id: &str,
        actor: &Principal,
    ) -> Result<Value> {
        ensure!(
            matches!(collection, "drafts" | "recipes"),
            "Unknown workflow collection"
        );
        let folder = self.folder(project)?;
        self.accounts.require_project(
            actor,
            &folder,
            if collection == "drafts" {
                "datasets.generate"
            } else {
                "project.manage"
            },
        )?;
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        fs::remove_file(self.workflow_path(project, collection, id)?)?;
        Ok(json!({"ok":true}))
    }

    pub fn queue_preparation(
        &self,
        project: &str,
        request: &Value,
        actor: &Principal,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        self.accounts
            .require_project(actor, &folder, "project.manage")?;
        let recording = field(request, "recording_uuid")?;
        let source = self.recording_file(project, recording, true, false)?;
        let topics = self.translation_identities(project, recording)?;
        let selected: Vec<String> = serde_json::from_value(request["selected_channels"].clone())?;
        ensure!(!selected.is_empty(), "Keep at least one topic");
        let valid: BTreeSet<_> = topics
            .as_array()
            .context("Missing topics")?
            .iter()
            .filter_map(|t| t["channel_id"].as_str())
            .collect();
        ensure!(
            selected.iter().all(|s| valid.contains(s.as_str())),
            "A selected topic is missing"
        );
        for (channel, script) in request["translations"].as_object().into_iter().flatten() {
            ensure!(
                selected.contains(channel),
                "Translations must target retained topics"
            );
            self.preview_topic_translation(
                project,
                recording,
                &json!({"channel_id":channel,"script":script}),
            )?;
        }
        let id = Uuid::new_v4().to_string();
        let mut recipe = request.clone();
        recipe["preserve_untranslated"] = json!(request["sync"]["enabled"] != true);
        recipe["source_sha256"] = source.item["sha256"].clone();
        let job = json!({"uuid":id,"project_uuid":project,"recording_uuid":recording,"source_file":source.item["source_file"],"status":"queued","stage":"Waiting to prepare","created_at":now(),"updated_at":now(),"created_by":actor.name,"request":recipe});
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        ensure!(
            load_manifest(&folder)?["mcaps"].get(recording).is_some(),
            "Recording was removed"
        );
        write_json(&self.workflow_path(project, "jobs", &id)?, &job)?;
        Ok(job)
    }

    pub fn run_next_preparation(&self, recover: bool) -> Result<bool> {
        for project in self.list_projects()?.as_array().into_iter().flatten() {
            let project_id = field(project, "uuid")?;
            for mut job in self
                .workflow_items(project_id, "jobs")?
                .as_array()
                .into_iter()
                .flatten()
                .cloned()
            {
                if recover && job["status"] == "running" {
                    job["status"] = json!("queued");
                    job["stage"] = json!("Resuming after restart");
                    write_json(
                        &self.workflow_path(project_id, "jobs", field(&job, "uuid")?)?,
                        &job,
                    )?;
                }
                if job["status"] != "queued" {
                    continue;
                }
                let path = self.workflow_path(project_id, "jobs", field(&job, "uuid")?)?;
                job["status"] = json!("running");
                job["stage"] = json!("Filtering, translating and building previews");
                job["updated_at"] = json!(now());
                write_json(&path, &job)?;
                let progress_job = job.clone();
                match self.pipeline_with_progress(project_id, &job["request"], |stage| {
                    let mut update = progress_job.clone();
                    update["stage"] = json!(stage);
                    update["updated_at"] = json!(now());
                    write_json(&path, &update)
                }) {
                    Ok(output) => {
                        job["status"] = json!("ready");
                        job["stage"] = json!("Ready to label");
                        job["output_recording"] = output["uuid"].clone();
                    }
                    Err(error) => {
                        job["status"] = json!("failed");
                        job["stage"] = json!("Preparation needs attention");
                        job["error"] = json!(format!("{error:#}"));
                    }
                }
                job["updated_at"] = json!(now());
                write_json(&path, &job)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn server_recordings(
        &self,
        project: &str,
        relative: &str,
        actor: &Principal,
    ) -> Result<Value> {
        self.accounts
            .require_project(actor, &self.folder(project)?, "project.manage")?;
        let root = std::env::var_os("MCAP_IMPORT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace.join("imports"));
        fs::create_dir_all(&root)?;
        let directory = if relative.is_empty() {
            root.clone()
        } else {
            contained_path(&root, relative)?
        };
        let mut entries = vec![];
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let child = if relative.is_empty() {
                name.to_string()
            } else {
                format!("{relative}/{name}")
            };
            let Ok(safe) = contained_path(&root, &child) else {
                continue;
            };
            if safe.is_dir()
                || safe
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("mcap"))
            {
                entries.push(json!({"name":name,"path":child,"directory":safe.is_dir(),"bytes":safe.metadata()?.len()}));
            }
        }
        entries.sort_by_key(|e| e["name"].as_str().unwrap_or_default().to_lowercase());
        Ok(json!({"root":root,"path":relative,"entries":entries}))
    }

    pub fn import_server_recording(
        &self,
        project: &str,
        relative: &str,
        actor: &Principal,
    ) -> Result<Value> {
        self.accounts
            .require_project(actor, &self.folder(project)?, "project.manage")?;
        let root = std::env::var_os("MCAP_IMPORT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace.join("imports"));
        let path = contained_path(&root, relative)?;
        self.import_recording_file(
            project,
            path.file_name()
                .and_then(|s| s.to_str())
                .context("Invalid filename")?,
            &path,
            None,
            None,
            true,
        )
    }

    pub(super) fn ensure_recording_unused(&self, project: &str, recording: &str) -> Result<()> {
        for p in self.list_projects()?.as_array().into_iter().flatten() {
            let id = field(p, "uuid")?;
            for draft in self
                .workflow_items(id, "drafts")?
                .as_array()
                .into_iter()
                .flatten()
            {
                ensure!(
                    !draft["recordings"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|r| r["project_uuid"] == project && r["recording_id"] == recording),
                    "Remove this recording from dataset draft '{}' first",
                    draft["name"].as_str().unwrap_or("Untitled dataset")
                );
            }
            if id == project {
                for job in self
                    .workflow_items(id, "jobs")?
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    ensure!(
                        !(job["recording_uuid"] == recording
                            && (job["status"] == "queued" || job["status"] == "running")),
                        "Wait for recording preparation to finish before removing its source"
                    );
                }
            }
            let manifest = self.cached_manifest(&self.folder(id)?)?;
            for dataset in manifest["datasets"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(_, d)| d)
            {
                ensure!(
                    !dataset["all_source_recordings"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|r| r == recording),
                    "A preserved dataset version still references this recording"
                );
            }
        }
        Ok(())
    }

    pub fn set_recording_session(
        &self,
        project: &str,
        request: &Value,
        actor: &Principal,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        self.accounts
            .require_project(actor, &folder, "project.manage")?;
        let session = field(request, "session_id")?;
        ensure!(
            !session.trim().is_empty()
                && session.len() <= 120
                && !session.chars().any(char::is_control),
            "Choose a capture session name of 1–120 characters"
        );
        let ids: Vec<String> = serde_json::from_value(request["recordings"].clone())?;
        ensure!(
            !ids.is_empty() && ids.len() <= 10000,
            "Choose recordings to group"
        );
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = load_manifest(&folder)?;
        Self::editable(&manifest)?;
        let before = manifest.clone();
        let mut roots = BTreeSet::new();
        for id in &ids {
            canonical_uuid(id)?;
            let item = manifest["mcaps"].get(id).context("Recording not found")?;
            roots.insert(item["source_root_uuid"].as_str().unwrap_or(id).to_string());
        }
        for (id, item) in manifest["mcaps"]
            .as_object_mut()
            .context("Missing recordings")?
        {
            if roots.contains(item["source_root_uuid"].as_str().unwrap_or(id)) {
                item["session_id"] = json!(session.trim());
            }
        }
        crate::storage::save_manifest_delta(&folder, &manifest, &before)?;
        Ok(self.detail(&folder, &manifest))
    }
}
