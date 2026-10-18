//! Project lifecycle, discovery, summaries and format upgrades.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    collections::HashSet,
    fs::{self},
    path::{Path, PathBuf},
};
use uuid::Uuid;

use super::{
    contained_path, field, load_manifest, now, read_json, registry, valid_name, write_json, Store,
};

impl Store {
    pub fn list_projects(&self) -> Result<Value> {
        self.discover()?;
        let folders: Vec<PathBuf> = self
            .registry
            .lock()
            .map_err(|_| anyhow::anyhow!("Registry unavailable"))?
            .values()
            .cloned()
            .collect();
        let mut projects = Vec::new();
        for folder in folders {
            if let Ok(project) = self.cached_manifest(&folder) {
                projects.push(json!({"uuid":project["uuid"], "name":project["name"], "path":folder, "recording_count":if project["format_version"] == 1 {1} else {project["mcaps"].as_object().map_or(0,Map::len)}, "label_count":project["labels"].as_object().map_or(0,Map::len), "dataset_count":project["datasets"].as_object().map_or(0,Map::len)}));
            }
        }
        projects.sort_by_key(|p| p["name"].as_str().unwrap_or("").to_lowercase());
        Ok(json!(projects))
    }

    pub(super) fn detail(&self, folder: &Path, project: &Value) -> Value {
        let mut detail = project.clone();
        detail["path"] = json!(folder);
        for name in ["mcaps", "labels", "label_lists", "datasets"] {
            let items: Vec<Value> = project[name]
                .as_object()
                .map(|items| {
                    items
                        .values()
                        .cloned()
                        .map(|mut item| {
                            if name == "datasets" {
                                let metadata = self.dataset_metadata(folder, project, &item);
                                item.as_object_mut()
                                    .unwrap()
                                    .extend(metadata.as_object().unwrap().clone());
                                if let Some(entries) = item["entries"].as_object() {
                                    item["entry_count"] = json!(entries.len());
                                }
                                item.as_object_mut().unwrap().remove("entries");
                                item.as_object_mut().unwrap().remove("entry_pages");
                                item.as_object_mut().unwrap().remove("sample_pages");
                            }
                            item
                        })
                        .collect()
                })
                .unwrap_or_default();
            detail[name] = json!(items);
        }
        if project["format_version"] == 1 {
            detail["mcaps"] = json!([{"uuid":project["uuid"],"source_file":project["source_file"],"path":project["mcap"],"sha256":project["sha256"]}]);
        }
        let published = self.recording_dataset_usages(project);
        if let Some(items) = detail["mcaps"].as_array_mut() {
            for item in items {
                item["dataset_usage_available"] = json!(published.is_ok());
                item["dataset_usages"] = published
                    .as_ref()
                    .ok()
                    .and_then(|usages| usages.get(item["uuid"].as_str().unwrap_or("")))
                    .cloned()
                    .unwrap_or(json!([]));
                if let Some(metadata) = self.recording_file_metadata(folder, item) {
                    item.as_object_mut()
                        .unwrap()
                        .extend(metadata.as_object().unwrap().clone());
                }
                if let Some(summary) = self.recording_summary(folder, project, item) {
                    item["summary"] = summary;
                }
            }
        }
        detail
    }

    fn recording_dataset_usages(
        &self,
        project: &Value,
    ) -> Result<std::collections::HashMap<String, Value>> {
        let mut usages = std::collections::HashMap::<String, Value>::new();
        for dataset in project["datasets"]
            .as_object()
            .into_iter()
            .flat_map(|items| items.values())
            .filter(|item| item["kind"] == "approved-dataset")
        {
            for descriptor in dataset["versions"].as_array().into_iter().flatten() {
                let number = descriptor["number"]
                    .as_u64()
                    .context("Invalid published version")?;
                let version =
                    self.dataset_version(field(project, "uuid")?, field(dataset, "uuid")?, number)?;
                for source in version["source_revisions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|source| source["project_uuid"] == project["uuid"])
                {
                    let recording = field(source, "recording_id")?.to_owned();
                    usages.entry(recording).or_insert(json!([])).as_array_mut().unwrap().push(json!({"dataset_id":dataset["uuid"],"name":dataset["name"],"version":number,"revision":source["revision"],"task_id":source["task_id"]}));
                }
            }
        }
        Ok(usages)
    }

    fn recording_file_metadata(&self, folder: &Path, item: &Value) -> Option<Value> {
        let path = contained_path(folder, item["path"].as_str()?).ok()?;
        let stamp = crate::storage::fingerprint(&path).ok()?;
        let cache = contained_path(
            folder,
            &format!("indexes/{}/recording-metadata.json", item["uuid"].as_str()?),
        )
        .ok()?;
        if let Ok(saved) = self.cached_json(&cache) {
            if saved["fingerprint"] == stamp && saved["metadata_version"] == 2 {
                return Some(saved["metadata"].clone());
            }
        }
        let metadata = match crate::engine::recording_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => return Some(json!({"file_size_bytes":stamp["size"]})),
        };
        let _ = write_json(
            &cache,
            &json!({"metadata_version":2,"fingerprint":stamp,"metadata":metadata}),
        );
        Some(metadata)
    }

    // Summaries use existing indexes; opening a project never decodes recording data.
    pub(super) fn recording_summary(
        &self,
        folder: &Path,
        project: &Value,
        item: &Value,
    ) -> Option<Value> {
        let path = contained_path(folder, item["path"].as_str()?).ok()?;
        let original = if project["format_version"] == 1 {
            folder
        } else {
            path.parent()?
        };
        let cached = format!(
            "data/cache/{}/{}",
            project["uuid"].as_str()?,
            item["uuid"].as_str()?
        );
        let index_path = contained_path(original, "frames/index.json")
            .ok()
            .filter(|path| path.is_file())
            .or_else(|| {
                contained_path(&self.workspace, &format!("{cached}/frames/index.json")).ok()
            })?;
        let annotations_path = if project["format_version"] == 3 {
            contained_path(
                folder,
                &crate::storage::annotation_path(item["uuid"].as_str()?),
            )
            .ok()?
        } else {
            contained_path(original, "annotations.json").ok()?
        };
        let dependencies = json!({"summary_version":3,"index":crate::storage::fingerprint(&index_path).ok()?,"annotations":crate::storage::fingerprint(&annotations_path).ok(),"sha256":item["sha256"]});
        let summary_path = contained_path(
            folder,
            &format!("indexes/{}/summary.json", item["uuid"].as_str()?),
        )
        .ok()?;
        if let Ok(saved) = read_json(&summary_path) {
            if saved["dependencies"] == dependencies {
                return Some(saved["summary"].clone());
            }
        }
        let index = self.cached_json(&index_path).ok()?;
        if index["sha256"] != item["sha256"] {
            return None;
        }
        let topics = index["topics"].as_object()?;
        let mut start: Option<u64> = None;
        let mut end: Option<u64> = None;
        let mut frame_count = 0;
        let mut modalities = HashSet::new();
        let mut topic_names = HashSet::new();
        for topic in topics.values() {
            if let Some(kind) = topic["kind"].as_str() {
                modalities.insert(kind.to_owned());
            }
            if let Some(name) = topic["topic"].as_str() {
                topic_names.insert(name.to_owned());
            }
            if let Some(frames) = topic["frames"].as_array() {
                frame_count += frames.len();
                for frame in frames {
                    let time = frame["time_ns"]
                        .as_u64()
                        .or_else(|| frame["time_ns"].as_str()?.parse().ok());
                    if let Some(time) = time {
                        start = Some(start.map_or(time, |value| value.min(time)));
                        end = Some(end.map_or(time, |value| value.max(time)));
                    }
                }
            }
        }
        let annotations = if project["format_version"] == 3 {
            crate::storage::read_annotations(folder, item["uuid"].as_str()?)
                .ok()
                .flatten()
        } else {
            read_json(&annotations_path).ok()
        }
        .filter(|state| state["sha256"] == item["sha256"]);
        let mut annotated_count = 0;
        let mut reviewed_count = 0;
        let mut coverage = Map::<String, Value>::new();
        for (channel, topic) in topics {
            let required = topic["kind"] == "image"
                || annotations.as_ref().is_some_and(|state| {
                    matches!(
                        state["techniques"][channel].as_str(),
                        Some("yolo" | "classification" | "csv")
                    )
                });
            coverage.insert(channel.clone(),json!({"frame_count":topic["frames"].as_array().map_or(0,Vec::len),"annotated_count":0,"required":required}));
        }
        if let Some(channels) = annotations
            .as_ref()
            .and_then(|state| state["annotations"].as_object())
        {
            for (channel, rows) in channels {
                let count = topics
                    .get(channel)
                    .and_then(|topic| topic["frames"].as_array())
                    .map_or(0, Vec::len);
                if let Some(rows) = rows.as_object() {
                    for (number, row) in rows {
                        if row["skipped"] == true {
                            continue;
                        }
                        if !number.parse::<usize>().is_ok_and(|number| number < count) {
                            continue;
                        }
                        if row["reviewed"] == true {
                            reviewed_count += 1;
                        }
                        if row["reviewed"] == true
                            || row["boxes"]
                                .as_array()
                                .is_some_and(|items| !items.is_empty())
                            || row["labels"]
                                .as_array()
                                .is_some_and(|items| !items.is_empty())
                            || (!row["fields"].is_null() && row["fields"] != json!({}))
                        {
                            if let Some(topic) = coverage.get_mut(channel) {
                                topic["annotated_count"] =
                                    json!(topic["annotated_count"].as_u64().unwrap_or(0) + 1);
                            }
                        }
                        if row["boxes"]
                            .as_array()
                            .is_some_and(|items| !items.is_empty())
                            || row["labels"]
                                .as_array()
                                .is_some_and(|items| !items.is_empty())
                            || (!row["fields"].is_null() && row["fields"] != json!({}))
                        {
                            annotated_count += 1;
                        }
                    }
                }
            }
        }
        let mut modalities: Vec<_> = modalities.into_iter().collect();
        modalities.sort();
        let mut topic_names: Vec<_> = topic_names.into_iter().collect();
        topic_names.sort();
        let summary = json!({"topic_count":topics.len(), "topic_names":topic_names,"start_time_ns":start.map(|s|s.to_string()),"end_time_ns":end.map(|s|s.to_string()), "duration_seconds":end.zip(start).map_or(0.0, |(end, start)| (end - start) as f64 / 1e9), "modalities":modalities, "frame_count":frame_count, "annotated_count":annotated_count, "reviewed_count":reviewed_count,"annotation_topics":coverage});
        let _ = crate::storage::write_changed(
            &summary_path,
            &json!({"dependencies":dependencies,"summary":summary}),
        );
        Some(summary)
    }

    pub fn project(&self, id: &str) -> Result<Value> {
        let (folder, manifest) = self.manifest(id)?;
        Ok(self.detail(&folder, &manifest))
    }

    pub fn create_project(&self, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let name = valid_name(request)?;
        let safe_projects_root = contained_path(&self.workspace, "data/projects")?;
        fs::create_dir_all(&safe_projects_root)?;
        let projects_root = safe_projects_root.canonicalize()?;
        ensure!(
            projects_root.starts_with(&self.workspace),
            "Project storage escapes workspace"
        );
        let id = Uuid::new_v4().to_string();
        let folder = projects_root.join(&id);
        let temporary = tempfile::Builder::new()
            .prefix(".project-")
            .tempdir_in(&projects_root)?;
        for directory in [
            "mcap",
            "labels",
            "translations",
            "synchronization",
            "annotations",
            "datasets",
            "cache",
            "indexes",
            "exports",
        ] {
            fs::create_dir(temporary.path().join(directory))?;
        }
        let version = request["format_version"].as_u64().unwrap_or(3);
        ensure!(matches!(version, 2 | 3), "Create a version 2 or 3 project");
        let manifest = json!({"kind":"mcap-labeling-project","format_version":version,"uuid":id,"name":name,"created_at":now(),"mcaps":{},"labels":{},"label_lists":{},"datasets":{},"tasks":{}});
        crate::storage::save_manifest(temporary.path(), &manifest)?;
        fs::rename(temporary.path(), &folder)?;
        self.registry
            .lock()
            .map_err(|_| anyhow::anyhow!("Registry unavailable"))?
            .insert(id, folder.clone());
        Ok(self.detail(&folder, &manifest))
    }

    /// Browse local folders without registering a project or reading file contents.
    pub fn browse_projects(&self, request: &Value) -> Result<Value> {
        // A WSL backend should offer the Windows user's desktop alongside its own home.
        let desktop_home = self.workspace.ancestors().find(|path| {
            path.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "Users")
        });
        let home = desktop_home
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        let mut shortcuts = vec![json!({"name":"Workspace", "path":self.workspace})];
        if let Some(home) = home.filter(|path| path.is_dir()) {
            shortcuts.push(json!({"name":"Home", "path":home}));
            for desktop in [home.join("Desktop"), home.join("OneDrive/Desktop")] {
                if desktop.is_dir() {
                    shortcuts.push(json!({"name":"Desktop", "path":desktop}));
                }
            }
        }
        let folder = request["path"]
            .as_str()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace.clone())
            .canonicalize()
            .context("Selected folder does not exist")?;
        ensure!(folder.is_dir(), "Choose a folder to browse");
        let mut folders = Vec::new();
        for entry in fs::read_dir(&folder).context("Cannot browse this folder")? {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !entry.path().is_dir() {
                continue;
            }
            folders.push(json!({"name":name, "path":entry.path(), "is_project":entry.path().join("project.json").is_file()}));
        }
        folders.sort_by_cached_key(|item| {
            (
                item["is_project"] != true,
                item["name"].as_str().unwrap_or_default().to_lowercase(),
            )
        });
        Ok(
            json!({"path":folder, "parent":folder.parent(), "is_project":folder.join("project.json").is_file(), "folders":folders, "shortcuts":shortcuts}),
        )
    }

    pub fn open_project(&self, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let path = PathBuf::from(field(request, "path")?)
            .canonicalize()
            .context("Selected project folder does not exist")?;
        let folder =
            if path.is_file() && path.file_name().is_some_and(|name| name == "project.json") {
                path.parent().unwrap().to_owned()
            } else {
                path
            };
        let manifest = load_manifest(&folder)?;
        let id = field(&manifest, "uuid")?.to_owned();
        let mut entries = self
            .registry
            .lock()
            .map_err(|_| anyhow::anyhow!("Registry unavailable"))?;
        ensure!(
            entries.get(&id).is_none_or(|existing| existing == &folder),
            "A different folder has this project UUID"
        );
        entries.insert(id, folder.clone());
        let paths: Vec<Value> = entries.values().map(|path| json!(path)).collect();
        fs::create_dir_all(self.workspace.join("data"))?;
        write_json(
            &contained_path(&self.workspace, "data/opened-projects.json")?,
            &json!(paths),
        )?;
        Ok(self.detail(&folder, &manifest))
    }

    /// Upgrade metadata in place, retaining a legacy manifest for inspection.
    /// Recording bytes and frame caches are never copied or modified.
    pub fn upgrade_project(&self, id: &str) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = load_manifest(&folder)?;
        Self::editable(&manifest)?;
        if manifest["format_version"] == 3 {
            return Ok(self.detail(&folder, &manifest));
        }
        crate::storage::write_changed(
            &contained_path(&folder, "project.v2.backup.json")?,
            &manifest,
        )?;
        for (uuid, item) in registry(&manifest, "mcaps")? {
            let path = Path::new(field(item, "path")?)
                .parent()
                .context("Invalid recording path")?
                .join("annotations.json");
            let path = contained_path(&folder, &path.to_string_lossy())?;
            if path.is_file() {
                crate::storage::save_annotations(&folder, uuid, &read_json(&path)?)?;
            }
        }
        for dataset in manifest["datasets"]
            .as_object_mut()
            .context("Invalid datasets")?
            .values_mut()
        {
            for entry in dataset["entries"]
                .as_object_mut()
                .context("Invalid entries")?
                .values_mut()
            {
                if !entry["labeled_data"].is_null() {
                    entry["labeled_data"]["path"] =
                        json!(crate::storage::annotation_path(field(entry, "mcap_uuid")?));
                }
            }
        }
        manifest["format_version"] = json!(3);
        crate::storage::save_manifest(&folder, &manifest)?;
        Ok(self.detail(&folder, &manifest))
    }
}
