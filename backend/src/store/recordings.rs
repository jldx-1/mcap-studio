//! Recording import, preview reads, workflows and pipeline publication.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

use super::{
    canonical_uuid, contained_path, field, file_hash, load_manifest, now, read_json,
    refresh_classes, registry, stringify_timestamps, write_json, Recording, RecordingFile,
    RecordingSource, Store,
};

impl Store {
    pub fn import_recording(&self, id: &str, source_name: &str, source: &Path) -> Result<Value> {
        self.import_recording_file(id, source_name, source, None, None, false)
    }

    pub(crate) fn upload_file(&self, id: &str) -> Result<tempfile::NamedTempFile> {
        let (folder, manifest) = self
            .manifest(id)
            .context("Cannot resolve the upload project")?;
        Self::editable(&manifest)?;
        let parent = contained_path(&folder, "mcap")?;
        fs::create_dir_all(&parent).context("Cannot create the upload directory")?;
        tempfile::Builder::new()
            .prefix(".upload-")
            .tempfile_in(parent)
            .context("Cannot create a staged upload file")
    }

    pub(crate) fn import_uploaded_recording(
        &self,
        id: &str,
        name: &str,
        upload: tempfile::NamedTempFile,
        hash: String,
        defer_index: bool,
    ) -> Result<Value> {
        let source = upload.path().to_owned();
        self.import_recording_file(id, name, &source, Some(upload), Some(hash), defer_index)
    }

    pub(super) fn import_recording_file(
        &self,
        id: &str,
        source_name: &str,
        source: &Path,
        upload: Option<tempfile::NamedTempFile>,
        hash: Option<String>,
        defer_index: bool,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder).context("Cannot lock the project for import")?;
        let mut manifest = self
            .cached_manifest(&folder)
            .context("Cannot read the project for import")?;
        let previous = manifest.clone();
        Self::editable(&manifest)?;
        let source_name = Path::new(source_name)
            .file_name()
            .and_then(|name| name.to_str())
            .context("Invalid recording filename")?;
        ensure!(
            source_name.to_lowercase().ends_with(".mcap"),
            "Choose an MCAP file"
        );
        let uuid = Uuid::new_v4().to_string();
        let parent = contained_path(&folder, "mcap")?;
        fs::create_dir_all(&parent)?;
        let temporary = tempfile::Builder::new()
            .prefix(".recording-")
            .tempdir_in(&parent)
            .context("Cannot create the recording directory")?;
        let recording = temporary.path().join("recording.mcap");
        let sha256 = if let Some(upload) = upload {
            ensure!(
                upload.path().parent() == Some(parent.as_path()),
                "Upload must be staged inside this project"
            );
            upload
                .as_file()
                .sync_all()
                .context("Cannot synchronize the MCAP upload")?;
            upload
                .into_temp_path()
                .persist(&recording)
                .map_err(|error| error.error)
                .context("Cannot publish the staged MCAP upload")?;
            hash.context("Missing upload digest")?
        } else {
            // Copy and hash in one pass, retaining the user's original file.
            let mut reader = BufReader::new(File::open(source)?);
            let mut writer = BufWriter::new(File::create(&recording)?);
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 1024 * 1024];
            loop {
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                writer.write_all(&buffer[..count])?;
                digest.update(&buffer[..count]);
            }
            writer.flush()?;
            writer.get_ref().sync_all()?;
            format!("{:x}", digest.finalize())
        };
        let mut file = File::open(&recording).context("Cannot validate the uploaded MCAP")?;
        ensure!(
            file.metadata()?.len() >= (mcap::MAGIC.len() * 2) as u64,
            "Invalid MCAP file"
        );
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        ensure!(magic == mcap::MAGIC, "Invalid MCAP start magic");
        file.seek(SeekFrom::End(-8))?;
        file.read_exact(&mut magic)?;
        ensure!(magic == mcap::MAGIC, "Invalid MCAP end magic");
        // Windows/DrvFS cannot rename a directory while one of its files is open.
        drop(file);
        if let Some(existing) = registry(&manifest, "mcaps")?
            .values()
            .find(|item| item["sha256"] == sha256)
        {
            let mut result = if defer_index {
                existing.clone()
            } else {
                recording_detail(&self.recording_inner(id, field(existing, "uuid")?)?)
            };
            result["duplicate"] = json!(true);
            return Ok(result);
        }
        if !defer_index {
            crate::engine::build_index(&recording, temporary.path(), &sha256)?;
        }
        let item = json!({"uuid":uuid,"path":format!("mcap/{uuid}/recording.mcap"),"source_file":source_name,"sha256":sha256,"created_at":now(),"label_ids":registry(&manifest,"labels")?.keys().collect::<Vec<_>>()});
        let destination = contained_path(&folder, &format!("mcap/{uuid}"))?;
        fs::rename(temporary.path(), &destination)
            .context("Cannot publish the recording directory")?;
        manifest["mcaps"][&uuid] = item.clone();
        if let Err(error) = crate::storage::save_manifest_delta(&folder, &manifest, &previous) {
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }
        let source_path = destination.join("recording.mcap");
        let stamp = crate::storage::fingerprint(&source_path)?;
        self.hashes
            .lock()
            .map_err(|_| anyhow::anyhow!("Hash registry unavailable"))?
            .insert(source_path, (stamp.clone(), sha256.clone()));
        let _ = write_json(
            &contained_path(&folder, &format!("indexes/{uuid}/source.json"))?,
            &json!({"fingerprint":stamp,"sha256":sha256}),
        );
        if defer_index {
            return Ok(
                json!({"uuid":uuid,"path":item["path"],"source_file":source_name,"sha256":sha256,"created_at":item["created_at"],"indexing":"pending"}),
            );
        }
        self.recording_inner(id, &uuid)
            .map(|recording| recording_detail(&recording))
    }

    pub(super) fn recording_inner(&self, id: &str, recording_id: &str) -> Result<Recording> {
        self.read_recording(id, recording_id, false)
    }

    pub(super) fn recording_for_edit(&self, id: &str, recording_id: &str) -> Result<Recording> {
        self.read_recording(id, recording_id, true)
    }

    pub(super) fn read_recording(
        &self,
        id: &str,
        recording_id: &str,
        fresh: bool,
    ) -> Result<Recording> {
        self.read_recording_with_pages(id, recording_id, fresh, None)
    }

    pub(super) fn read_recording_with_pages(
        &self,
        id: &str,
        recording_id: &str,
        fresh: bool,
        buckets: Option<&HashSet<String>>,
    ) -> Result<Recording> {
        let source = self.recording_source(id, recording_id, fresh, false)?;
        let RecordingSource {
            project,
            item,
            folder,
            cache,
            original_cache,
            index,
        } = source;
        let manifest = (*project).clone();
        let annotations_path = contained_path(&original_cache, "annotations.json")?;
        let sharded = if manifest["format_version"] == 3 {
            if let Some(buckets) = buckets {
                crate::storage::read_annotation_subset(&folder, recording_id, buckets)?
            } else {
                crate::storage::read_annotations(&folder, recording_id)?
            }
        } else {
            None
        };
        let has_saved_annotations =
            sharded.is_some() || (manifest["format_version"] != 3 && annotations_path.is_file());
        let mut annotations = if let Some(state) = sharded {
            state
        } else if manifest["format_version"] != 3 && annotations_path.is_file() {
            self.cached_json(&annotations_path)?
        } else {
            let techniques: Map<String, Value> = registry(&index, "topics")?
                .iter()
                .map(|(id, topic)| {
                    (
                        id.clone(),
                        topic
                            .get("technique")
                            .cloned()
                            .unwrap_or(json!("undefined")),
                    )
                })
                .collect();
            json!({"format_version":1,"sha256":item["sha256"],"classes":[],"class_uuids":[],"techniques":techniques,"annotations":{},"settings":{"step":1,"positions":{}}})
        };
        ensure!(
            annotations["sha256"] == item["sha256"] && annotations["format_version"] == 1,
            "Saved annotations do not match recording"
        );
        for (channel, topic) in registry(&index, "topics")? {
            if topic["kind"] != "image"
                && annotations["techniques"][channel] == "classification"
                && annotations["annotations"][channel]
                    .as_object()
                    .is_none_or(Map::is_empty)
                && annotations["tracks"][channel]
                    .as_object()
                    .is_none_or(Map::is_empty)
            {
                annotations["techniques"][channel] = json!("csv");
            }
        }
        refresh_classes(&manifest, &mut annotations)?;
        if !has_saved_annotations && item["preparation_settings"].is_object() {
            for (source_channel, settings) in item["preparation_settings"]
                .as_object()
                .into_iter()
                .flatten()
            {
                if let Some(channel) =
                    item["pipeline_result"]["channel_id_map"][source_channel].as_u64()
                {
                    let channel = channel.to_string();
                    if let Some(technique) = settings["technique"].as_str() {
                        annotations["techniques"][&channel] = json!(technique);
                    }
                    if let Some(list_id) =
                        settings["label_list_id"].as_str().filter(|s| !s.is_empty())
                    {
                        if let Some(list) = manifest["label_lists"].get(list_id) {
                            if !annotations["topic_label_lists"].is_object() {
                                annotations["topic_label_lists"] = json!({});
                            }
                            if !annotations["topic_labels"].is_object() {
                                annotations["topic_labels"] = json!({});
                            }
                            annotations["topic_label_lists"][&channel] = json!(list_id);
                            annotations["topic_labels"][&channel] = list["label_ids"].clone();
                        }
                    }
                }
            }
        }
        Ok(Recording {
            project: manifest,
            item,
            folder,
            cache,
            index,
            annotations,
        })
    }

    pub(super) fn recording_file(
        &self,
        id: &str,
        recording_id: &str,
        fresh: bool,
        preview: bool,
    ) -> Result<RecordingFile> {
        canonical_uuid(recording_id)?;
        let folder = self.folder(id)?;
        let mut manifest = if preview {
            self.preview_manifest(&folder)?
        } else {
            self.shared_manifest(&folder)?
        };
        if fresh && manifest["format_version"] != 3 {
            manifest = Arc::new(load_manifest(&folder)?);
        }
        let item = if manifest["format_version"] == 1 && manifest["uuid"] == recording_id {
            json!({"uuid":recording_id,"path":manifest["mcap"],"sha256":manifest["sha256"],"source_file":manifest["source_file"]})
        } else if preview && manifest["format_version"] == 3 {
            ensure!(
                manifest["mcaps"].get(recording_id).is_some(),
                "Recording not found"
            );
            let item = self.shared_json(&contained_path(
                &folder,
                &crate::storage::metadata_path("mcaps", recording_id),
            )?)?;
            ensure!(
                item["uuid"] == recording_id,
                "Recording metadata UUID does not match registry"
            );
            (*item).clone()
        } else {
            manifest["mcaps"]
                .get(recording_id)
                .cloned()
                .context("Recording not found")?
        };
        let path = contained_path(&folder, field(&item, "path")?)?;
        if manifest["format_version"] != 1 {
            ensure!(
                path.parent() == Some(folder.join("mcap").join(recording_id).as_path())
                    && path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("mcap")),
                "Recording storage path does not match its own folder"
            );
        }
        let stamp = crate::storage::fingerprint(&path)?;
        let expected = field(&item, "sha256")?;
        let cached = self
            .hashes
            .lock()
            .map_err(|_| anyhow::anyhow!("Hash registry unavailable"))?
            .get(&path)
            .cloned();
        let verification = contained_path(&folder, &format!("indexes/{recording_id}/source.json"))?;
        if !cached.is_some_and(|(saved, hash)| saved == stamp && hash == expected) {
            let persisted = read_json(&verification).ok();
            let hash = if persisted
                .is_some_and(|saved| saved["fingerprint"] == stamp && saved["sha256"] == expected)
            {
                expected.to_owned()
            } else {
                file_hash(&path)?
            };
            ensure!(
                hash == expected,
                "Recording SHA-256 has changed; import again to avoid mismatched labels"
            );
            self.hashes
                .lock()
                .map_err(|_| anyhow::anyhow!("Hash registry unavailable"))?
                .insert(path.clone(), (stamp.clone(), hash.clone()));
            let _ = write_json(&verification, &json!({"fingerprint":stamp,"sha256":hash}));
        }
        Ok(RecordingFile {
            project: manifest,
            item,
            folder,
            path,
        })
    }

    pub(super) fn recording_source(
        &self,
        id: &str,
        recording_id: &str,
        fresh: bool,
        preview: bool,
    ) -> Result<RecordingSource> {
        let RecordingFile {
            project: manifest,
            item,
            folder,
            path,
        } = self.recording_file(id, recording_id, fresh, preview)?;
        let original_cache = if manifest["format_version"] == 1 {
            folder.clone()
        } else {
            path.parent().unwrap().to_owned()
        };
        let cache = if manifest["format_version"] == 3
            || original_cache.join("frames/index.json").exists()
        {
            original_cache.clone()
        } else {
            let relative = format!("data/cache/{id}/{recording_id}");
            contained_path(&self.workspace, &relative)?
        };
        let index_path = cache.join("frames/index.json");
        let needs_index = || -> Result<bool> {
            if !index_path.is_file() {
                return Ok(true);
            }
            let index = self.shared_json(&index_path)?;
            Ok(crate::engine::image_cache_needs_upgrade(&index))
        };
        if needs_index()? {
            let _guard = self
                .cache_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("Frame cache unavailable"))?;
            if needs_index()? {
                fs::create_dir_all(&cache)?;
                crate::engine::build_index(&path, &cache, field(&item, "sha256")?)?;
            }
        }
        let index = self.shared_json(&contained_path(&cache, "frames/index.json")?)?;
        ensure!(
            index["sha256"] == item["sha256"]
                && index["format_version"] == 1
                && index["topics"].is_object(),
            "Frame cache does not match recording"
        );
        Ok(RecordingSource {
            project: manifest,
            item,
            folder,
            cache,
            original_cache,
            index,
        })
    }

    pub fn recording(&self, id: &str, recording_id: &str) -> Result<Value> {
        Ok(recording_detail(&self.recording_inner(id, recording_id)?))
    }

    pub fn frame(
        &self,
        id: &str,
        recording_id: &str,
        channel: &str,
        number: usize,
    ) -> Result<(Vec<u8>, String)> {
        let recording = self.recording_source(id, recording_id, false, true)?;
        let frame = indexed_frame(&recording.index, channel, number)?;
        let path = contained_path(
            &recording.cache,
            &format!("frames/{}", field(frame, "image")?),
        )?;
        let mime = match path.extension().and_then(|extension| extension.to_str()) {
            Some("jpg" | "jpeg") => "image/jpeg",
            _ => "image/png",
        };
        Ok((fs::read(path)?, mime.to_owned()))
    }

    pub fn record(
        &self,
        id: &str,
        recording_id: &str,
        channel: &str,
        number: usize,
    ) -> Result<Value> {
        let recording = self.recording_source(id, recording_id, false, true)?;
        let mut record =
            decoded_record_from_index(&recording.index, &recording.cache, channel, number)?;
        stringify_timestamps(&mut record);
        Ok(record)
    }

    pub fn save_workflow(&self, id: &str, recording_id: &str, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let recording = self.recording_source(id, recording_id, true, false)?;
        Self::editable(&recording.project)?;
        validate_workflow(&recording.index, request)?;
        let relative = format!("mcap/{recording_id}/web-workflow.json");
        crate::storage::write_changed(&contained_path(&folder, &relative)?, request)?;
        let mut manifest = (*recording.project).clone();
        let previous = manifest.clone();
        manifest["mcaps"][recording_id]["web_workflow"] = json!(relative);
        if manifest["format_version"] == 3 {
            let translation = format!("translations/{recording_id}.json");
            let synchronization = format!("synchronization/{recording_id}.json");
            crate::storage::write_changed(
                &contained_path(&folder, &translation)?,
                &request["translations"],
            )?;
            crate::storage::write_changed(
                &contained_path(&folder, &synchronization)?,
                &request["sync"],
            )?;
            manifest["mcaps"][recording_id]["translation"] = json!(translation);
            manifest["mcaps"][recording_id]["synchronization"] = json!(synchronization);
        }
        crate::storage::save_manifest_delta(&folder, &manifest, &previous)?;
        Ok(request.clone())
    }

    pub fn pipeline(&self, id: &str, request: &Value) -> Result<Value> {
        self.pipeline_with_progress(id, request, |_| Ok(()))
    }

    pub(super) fn pipeline_with_progress(
        &self,
        id: &str,
        request: &Value,
        progress: impl Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        let recording_id = field(request, "recording_uuid")?;
        let folder = self.folder(id)?;
        let source = self.recording_file(id, recording_id, true, false)?;
        Self::editable(&source.project)?;
        if let Some(hash) = request.get("source_sha256") {
            ensure!(
                *hash == source.item["sha256"],
                "The source recording changed since preparation was queued"
            );
        }
        if request["preserve_untranslated"] == true {
            ensure!(
                request["sync"]["enabled"] != true,
                "Translation-only processing cannot synchronize topics"
            );
            let topics = self.translation_identities(id, recording_id)?;
            let index = json!({"topics":topics.as_array().context("Missing translation topics")?.iter().map(|topic| (topic["channel_id"].as_str().unwrap_or_default().to_owned(),topic.clone())).collect::<Map<_,_>>()});
            validate_workflow(&index, request)?;
            // Repeated application of the same scripts reuses the immutable processed recording.
            for item in registry(&source.project, "mcaps")?.values() {
                if item["source_recording_uuid"] == recording_id {
                    if let Some(provenance) = item["pipeline_provenance"].as_str() {
                        if read_json(&contained_path(&folder, provenance)?)
                            .is_ok_and(|saved| saved == *request)
                        {
                            return self.recording(id, field(item, "uuid")?);
                        }
                    }
                }
            }
        } else {
            validate_workflow(&self.recording_for_edit(id, recording_id)?.index, request)?;
        }
        let uuid = Uuid::new_v4().to_string();
        let parent = contained_path(&folder, "mcap")?;
        let temporary = tempfile::Builder::new()
            .prefix(".recording-")
            .tempdir_in(&parent)?;
        let source_path = source.path.clone();
        let output = temporary.path().join("recording.mcap");
        progress("Filtering and translating selected topics")?;
        let result = crate::engine::process_recording(&source_path, &output, request)?;
        progress("Building previews and recording index")?;
        let index = crate::engine::build_index_and_hash(&output, temporary.path())?;
        let hash = field(&index, "sha256")?.to_owned();
        let applied_workflow = json!({"selected_channels":index["topics"].as_object().context("Processed recording needs topics")?.keys().collect::<Vec<_>>(),"translations":{},"sync":{"enabled":false,"parent_channel":"","mode":"nearest","tolerance_ns":null,"delete_out_of_sync":false}});
        write_json(
            &temporary.path().join("web-workflow.json"),
            &applied_workflow,
        )?;
        write_json(&temporary.path().join("pipeline.json"), request)?;
        let mut item = json!({"uuid":uuid,"path":format!("mcap/{uuid}/recording.mcap"),"source_file":format!("processed-{}",source.item["source_file"].as_str().unwrap_or("recording.mcap")),"sha256":hash,"created_at":now(),"label_ids":source.item["label_ids"],"source_recording_uuid":recording_id,"source_root_uuid":source.item.get("source_root_uuid").unwrap_or(&json!(recording_id)),"session_id":source.item.get("session_id").unwrap_or(&json!(recording_id)),"pipeline_result":result,"pipeline_provenance":format!("mcap/{uuid}/pipeline.json"),"web_workflow":format!("mcap/{uuid}/web-workflow.json"),"preparation_settings":request["topic_settings"]});
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = load_manifest(&folder)?;
        Self::editable(&manifest)?;
        ensure!(
            manifest["mcaps"][recording_id]["sha256"] == source.item["sha256"],
            "The source recording was removed or changed during preparation"
        );
        if let Some(session) = manifest["mcaps"][recording_id]["session_id"].as_str() {
            item["session_id"] = json!(session);
        }
        if request["preserve_untranslated"] == true {
            for existing in registry(&manifest, "mcaps")?.values() {
                if existing["source_recording_uuid"] == recording_id {
                    if let Some(path) = existing["pipeline_provenance"].as_str() {
                        if read_json(&contained_path(&folder, path)?)
                            .is_ok_and(|saved| saved == *request)
                        {
                            return self
                                .recording_inner(id, field(existing, "uuid")?)
                                .map(|r| recording_detail(&r));
                        }
                    }
                }
            }
        }
        let destination = contained_path(&folder, &format!("mcap/{uuid}"))?;
        progress("Publishing prepared recording")?;
        fs::rename(temporary.path(), &destination)?;
        manifest["mcaps"][&uuid] = item;
        if let Err(error) = crate::storage::save_manifest(&folder, &manifest) {
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }
        // The digest was calculated before publication; avoid hashing the new output again.
        let output_path = destination.join("recording.mcap");
        let stamp = crate::storage::fingerprint(&output_path)?;
        self.hashes
            .lock()
            .map_err(|_| anyhow::anyhow!("Hash registry unavailable"))?
            .insert(output_path, (stamp.clone(), hash.clone()));
        let _ = write_json(
            &contained_path(&folder, &format!("indexes/{uuid}/source.json"))?,
            &json!({"fingerprint":stamp,"sha256":hash}),
        );
        Ok(recording_detail(&self.recording_inner(id, &uuid)?))
    }

    pub fn delete_recording(&self, id: &str, recording_id: &str) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        canonical_uuid(recording_id)?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = load_manifest(&folder)?;
        Self::editable(&manifest)?;
        let item = manifest["mcaps"]
            .get(recording_id)
            .context("Recording not found")?;
        self.ensure_recording_unused(id, recording_id)?;
        let dependencies: Vec<_> = registry(&manifest, "datasets")?
            .values()
            .filter(|dataset| {
                dataset["source_recording_uuid"] == recording_id
                    || dataset["all_source_recordings"]
                        .as_array()
                        .is_some_and(|rows| rows.iter().any(|r| r == recording_id))
                    || dataset["sources"].as_array().is_some_and(|sources| {
                        sources
                            .iter()
                            .any(|source| source["mcap_uuid"] == recording_id)
                    })
                    || dataset["entries"].as_object().is_some_and(|entries| {
                        entries
                            .values()
                            .any(|entry| entry["mcap_uuid"] == recording_id)
                    })
            })
            .map(|dataset| dataset["name"].as_str().unwrap_or("Dataset"))
            .collect();
        ensure!(
            dependencies.is_empty(),
            "Remove the datasets referencing this MCAP first: {}",
            dependencies.join(", ")
        );
        let recording_path = contained_path(&folder, field(item, "path")?)?;
        let directory = contained_path(&folder, &format!("mcap/{recording_id}"))?;
        ensure!(
            directory == folder.join(format!("mcap/{recording_id}"))
                && recording_path.parent() == Some(directory.as_path()),
            "Recording storage path does not match its own folder"
        );
        let cache = contained_path(&self.workspace, &format!("data/cache/{id}/{recording_id}"))?;
        ensure!(
            cache
                == self
                    .workspace
                    .join(format!("data/cache/{id}/{recording_id}")),
            "Recording cache path does not match its own folder"
        );
        let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
        for path in [directory, cache] {
            if !path.is_dir() {
                continue;
            }
            let retired = path
                .parent()
                .unwrap()
                .join(format!(".deleted-{}", Uuid::new_v4()));
            if let Err(error) = fs::rename(&path, &retired) {
                for (original, retired) in staged.iter().rev() {
                    let _ = fs::rename(retired, original);
                }
                return Err(error.into());
            }
            staged.push((path, retired));
        }
        manifest["mcaps"]
            .as_object_mut()
            .context("Invalid recording registry")?
            .remove(recording_id);
        if let Err(error) = crate::storage::save_manifest(&folder, &manifest) {
            for (original, retired) in staged.iter().rev() {
                let _ = fs::rename(retired, original);
            }
            return Err(error);
        }
        for (_, retired) in staged {
            fs::remove_dir_all(retired)?;
        }
        if let Ok(mut hashes) = self.hashes.lock() {
            hashes.remove(&recording_path);
        }
        Ok(self.detail(&folder, &manifest))
    }
}

pub(super) fn recording_detail(recording: &Recording) -> Value {
    let topics:Vec<Value>=recording.index["topics"].as_object().unwrap().iter().map(|(channel,topic)|json!({"channel_id":channel,"topic":topic["topic"],"kind":topic["kind"],"encoding":topic["encoding"],"schema_name":topic.get("schema_name").or(topic.get("schema")).cloned().unwrap_or(Value::Null),"frame_count":topic["frames"].as_array().map_or(0,Vec::len),"technique":recording.annotations["techniques"][channel],"errors":topic["errors"],"unsupported":topic["unsupported"]})).collect();
    let workflow = recording_workflow(recording);
    let mut result = recording.item.clone();
    let mut index = (*recording.index).clone();
    stringify_timestamps(&mut index);
    result["topics"] = json!(topics);
    result["index"] = index;
    result["annotations"] = recording.annotations.clone();
    result["workflow"] = workflow;
    result
}

pub(super) fn recording_workflow(recording: &Recording) -> Value {
    let mut workflow = json!({"selected_channels":recording.index["topics"].as_object().unwrap().keys().collect::<Vec<_>>(),"translations":{},"sync":{"enabled":false,"parent_channel":"","mode":"nearest","tolerance_ns":null,"delete_out_of_sync":false}});
    if let Some(relative) = recording.item["web_workflow"].as_str() {
        if let Ok(path) = contained_path(&recording.folder, relative) {
            if let Ok(saved) = read_json(&path) {
                workflow = saved;
            }
        }
    }
    if recording.project["format_version"] == 3 {
        for (key, target) in [("translation", "translations"), ("synchronization", "sync")] {
            if let Some(relative) = recording.item[key].as_str() {
                if let Ok(saved) =
                    contained_path(&recording.folder, relative).and_then(|path| read_json(&path))
                {
                    workflow[target] = saved;
                }
            }
        }
    }
    workflow
}

pub(super) fn indexed_frame<'a>(
    index: &'a Value,
    channel: &str,
    number: usize,
) -> Result<&'a Value> {
    ensure!(channel.parse::<u16>().is_ok(), "Channel ID must be numeric");
    index["topics"].get(channel).context("Topic not found")?["frames"]
        .as_array()
        .context("Topic needs frames")?
        .get(number)
        .context("Frame is outside this topic")
}

pub(super) fn decoded_record(recording: &Recording, channel: &str, number: usize) -> Result<Value> {
    decoded_record_from_index(&recording.index, &recording.cache, channel, number)
}

pub(super) fn decoded_record_from_index(
    index: &Value,
    cache: &Path,
    channel: &str,
    number: usize,
) -> Result<Value> {
    let frame = indexed_frame(index, channel, number)?;
    let offset = frame["offset"]
        .as_u64()
        .context("This frame has no decoded record")?;
    let path = contained_path(cache, &format!("frames/channel_{channel}/records.jsonl"))?;
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut line = String::new();
    BufReader::new(file.take(16 * 1024 * 1024)).read_line(&mut line)?;
    serde_json::from_str(&line).context("Invalid decoded record")
}

pub(super) fn validate_workflow(index: &Value, request: &Value) -> Result<()> {
    let channels = request["selected_channels"]
        .as_array()
        .context("Select channels for the pipeline")?;
    ensure!(!channels.is_empty(), "Select at least one channel");
    let mut selected = HashSet::new();
    for channel in channels {
        let channel = channel.as_str().context("Channel IDs must be strings")?;
        ensure!(
            index["topics"].get(channel).is_some() && selected.insert(channel),
            "Select unique channels from this recording"
        );
    }
    if let Some(translations) = request["translations"].as_object() {
        for (channel, translation) in translations {
            ensure!(
                index["topics"].get(channel).is_some() && translation.is_string(),
                "Translations require a known channel and a declarative script"
            );
        }
    }
    if request["sync"]["enabled"] == true {
        let sync = &request["sync"];
        let parent = field(sync, "parent_channel")?;
        ensure!(
            selected.contains(parent),
            "Synchronization parent must be selected"
        );
        ensure!(
            matches!(
                sync["mode"].as_str(),
                Some("nearest" | "exact" | "interpolate" | "basic" | "linear" | "rate")
            ),
            "Unsupported synchronization mode"
        );
        ensure!(
            sync["tolerance_ns"].is_null() || sync["tolerance_ns"].as_u64().is_some(),
            "Tolerance must be null or a nonnegative integer in nanoseconds"
        );
    }
    Ok(())
}
