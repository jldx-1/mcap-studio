//! Portable project API, supporting legacy manifests and incremental folder projects.
use anyhow::{ensure, Context, Result};
use fs2::FileExt;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub struct Store {
    pub workspace: PathBuf,
    pub accounts: crate::accounts::Accounts,
    projects_root: PathBuf,
    registry: Mutex<BTreeMap<String, PathBuf>>,
    hashes: Mutex<HashMap<PathBuf, (Value, String)>>,
    manifests: Mutex<HashMap<PathBuf, ManifestSnapshot>>,
    json_cache: Mutex<HashMap<PathBuf, ManifestSnapshot>>,
    cache_gate: Mutex<()>,
    gate: Mutex<()>,
}

struct ManifestSnapshot {
    stamp: Value,
    manifest: Arc<Value>,
}

struct Recording {
    project: Value,
    item: Value,
    folder: PathBuf,
    cache: PathBuf,
    index: Arc<Value>,
    annotations: Value,
}

struct RecordingFile {
    project: Arc<Value>,
    item: Value,
    folder: PathBuf,
    path: PathBuf,
}

struct RecordingSource {
    project: Arc<Value>,
    item: Value,
    folder: PathBuf,
    cache: PathBuf,
    original_cache: PathBuf,
    index: Arc<Value>,
}

mod access;
mod annotations;
mod dataset_generation;
mod dataset_versions;
mod datasets;
mod exports;
mod labels;
mod manifest;
mod model;
mod projects;
mod recordings;
mod reviews;
mod structured;
mod translations;
mod workflow;
mod yolo;
pub(crate) use yolo::upload_name;

pub(crate) use crate::storage::read_json;
pub use crate::storage::{contained_path, file_hash, write_json};
use crate::storage::{copy_tree, list_files};
pub use annotations::validate_annotations;
use annotations::{refresh_classes, reviewed_rows, selected_annotation_channels};
use manifest::load_manifest;
pub use model::stringify_timestamps;
use model::{canonical_uuid, field, now, registry, same_json, valid_name};
use recordings::{decoded_record, indexed_frame, recording_workflow};
use structured::{csv_cell, flatten_message};

impl Store {
    pub fn new(workspace: &Path) -> Result<Self> {
        fs::create_dir_all(workspace)?;
        let workspace = workspace.canonicalize()?;
        let projects_root = workspace.join("data/projects");
        let store = Self {
            accounts: crate::accounts::Accounts::new(&workspace)?,
            workspace,
            projects_root,
            registry: Mutex::new(BTreeMap::new()),
            hashes: Mutex::new(HashMap::new()),
            manifests: Mutex::new(HashMap::new()),
            json_cache: Mutex::new(HashMap::new()),
            cache_gate: Mutex::new(()),
            gate: Mutex::new(()),
        };
        store.discover()?;
        Ok(store)
    }

    fn discover(&self) -> Result<()> {
        let mut folders = vec![self.workspace.clone()];
        for parent in [&self.workspace, &self.projects_root] {
            if parent.is_dir() {
                folders.extend(
                    fs::read_dir(parent)?
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path())
                        .filter(|path| path.is_dir()),
                );
            }
        }
        let opened = self.workspace.join("data/opened-projects.json");
        if opened.is_file() {
            if let Ok(Value::Array(paths)) = read_json(&opened) {
                folders.extend(
                    paths
                        .into_iter()
                        .filter_map(|value| value.as_str().map(PathBuf::from)),
                );
            }
        }
        let mut entries = self
            .registry
            .lock()
            .map_err(|_| anyhow::anyhow!("Project registry lock is poisoned"))?;
        for folder in folders {
            if !folder.join("project.json").is_file() {
                continue;
            }
            if let Ok(manifest) = self.cached_manifest(&folder) {
                let folder = folder.canonicalize()?;
                let id = field(&manifest, "uuid")?.to_owned();
                if let Some(existing) = entries.get(&id) {
                    if existing != &folder {
                        tracing::warn!(%id, "Skipping duplicated project UUID");
                    }
                } else {
                    entries.insert(id, folder);
                }
            }
        }
        Ok(())
    }

    fn folder(&self, id: &str) -> Result<PathBuf> {
        canonical_uuid(id)?;
        self.registry
            .lock()
            .map_err(|_| anyhow::anyhow!("Registry unavailable"))?
            .get(id)
            .cloned()
            .context("Project not found")
    }

    pub(crate) fn project_folder(&self, id: &str) -> Result<PathBuf> {
        self.folder(id)
    }

    fn manifest(&self, id: &str) -> Result<(PathBuf, Value)> {
        let folder = self.folder(id)?;
        let manifest = self.cached_manifest(&folder)?;
        Ok((folder, manifest))
    }

    fn cached_manifest(&self, folder: &Path) -> Result<Value> {
        Ok((*self.shared_manifest(folder)?).clone())
    }

    fn shared_manifest(&self, folder: &Path) -> Result<Arc<Value>> {
        let folder = folder.canonicalize()?;
        let cached = self
            .manifests
            .lock()
            .map_err(|_| anyhow::anyhow!("Manifest registry unavailable"))?
            .get(&folder)
            .map(|saved| (saved.stamp.clone(), Arc::clone(&saved.manifest)));
        let stamp = if let Some((stamp, manifest)) = &cached {
            crate::storage::known_manifest_stamp(&folder, stamp, manifest)?
        } else {
            crate::storage::manifest_stamp(&folder)?
        };
        if let Some((saved, manifest)) = cached {
            if saved == stamp {
                return Ok(manifest);
            }
        }
        let manifest = Arc::new(load_manifest(&folder)?);
        self.manifests
            .lock()
            .map_err(|_| anyhow::anyhow!("Manifest registry unavailable"))?
            .insert(
                folder,
                ManifestSnapshot {
                    stamp,
                    manifest: manifest.clone(),
                },
            );
        Ok(manifest)
    }

    fn preview_manifest(&self, folder: &Path) -> Result<Arc<Value>> {
        let root_stamp = crate::storage::fingerprint(&contained_path(folder, "project.json")?)?;
        let cached = self
            .manifests
            .lock()
            .map_err(|_| anyhow::anyhow!("Manifest registry unavailable"))?
            .get(folder)
            .filter(|saved| saved.stamp["project"] == root_stamp)
            .map(|saved| Arc::clone(&saved.manifest));
        // Frames depend on this recording's metadata, not every label/dataset.
        if let Some(manifest) = cached {
            return Ok(manifest);
        }
        self.shared_manifest(folder)
    }

    fn editable(manifest: &Value) -> Result<()> {
        ensure!(
            matches!(manifest["format_version"].as_u64(), Some(2 | 3)),
            "This version 1 project is read-only; use a version 2 or 3 project for edits"
        );
        Ok(())
    }

    fn cached_json(&self, path: &Path) -> Result<Value> {
        Ok((*self.shared_json(path)?).clone())
    }

    fn shared_json(&self, path: &Path) -> Result<Arc<Value>> {
        let stamp = crate::storage::fingerprint(path)?;
        let mut cache = self
            .json_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("JSON cache unavailable"))?;
        if let Some(saved) = cache.get(path).filter(|saved| saved.stamp == stamp) {
            return Ok(saved.manifest.clone());
        }
        let value = Arc::new(read_json(path)?);
        // Keep caches bounded when browsing many recordings.
        if cache.len() >= 64 {
            cache.clear();
        }
        cache.insert(
            path.to_owned(),
            ManifestSnapshot {
                stamp,
                manifest: value.clone(),
            },
        );
        Ok(value)
    }

    fn lock_project(folder: &Path) -> Result<File> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(contained_path(folder, ".project.lock")?)?;
        lock.lock_exclusive()?;
        Ok(lock)
    }
}

mod translation_definitions;
