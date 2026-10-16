//! Named translation files matched by topic, message type, and schema fingerprint.
use super::{contained_path, field, now, read_json, valid_name, Store};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};

fn matches(profile: &Value, topic: &Value) -> bool {
    profile["format_version"] == 1
        && profile["source_message"] == topic["source_message"]
        && profile["source_schema_sha256"] == topic["source_schema_sha256"]
        && profile
            .get("topic")
            .is_none_or(|name| name == &topic["topic"])
}

fn preview_value(value: &Value) -> Value {
    match value {
        Value::Array(items) if items.len() > 30 => {
            json!({"items":items.len(),"first":items.iter().take(5).map(preview_value).collect::<Vec<_>>()})
        }
        Value::Array(items) => json!(items.iter().map(preview_value).collect::<Vec<_>>()),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(key, value)| (key.clone(), preview_value(value)))
                .collect(),
        ),
        Value::String(text) if text.len() > 300 => json!(format!(
            "{}… ({} characters)",
            text.chars().take(200).collect::<String>(),
            text.chars().count()
        )),
        _ => value.clone(),
    }
}

impl Store {
    fn translation_library(&self, project: &str) -> Result<Vec<Value>> {
        let folder = self.folder(project)?;
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_owned();
        let mut directories = vec![contained_path(&folder, "translations/library")?];
        for root in [&self.workspace, &repository] {
            for relative in [
                "utils/translations",
                "utils/mcap-data-pipeline/translations",
                "misc/mcap-data-pipeline/translations",
            ] {
                if let Ok(path) = contained_path(root, relative) {
                    if !directories.contains(&path) {
                        directories.push(path);
                    }
                }
            }
        }
        let mut profiles = BTreeMap::new();
        for directory in directories {
            if !directory.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                if !entry.file_type()?.is_file()
                    || entry.path().extension().is_none_or(|ext| ext != "json")
                    || entry.metadata()?.len() > 1024 * 1024
                {
                    continue;
                }
                let path = contained_path(&directory, &entry.file_name().to_string_lossy())?;
                let Ok(mut profile) = read_json(&path) else {
                    continue;
                };
                if !profile.is_object()
                    || profile["format_version"] != 1
                    || !profile["name"].is_string()
                {
                    continue;
                }
                let Ok(script) = crate::engine::translation_file_script(&profile) else {
                    continue;
                };
                let id = profile["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!("{:x}", Sha256::digest(fs::read(&path).unwrap_or_default()))
                    });
                profile["script"] = json!(script);
                profile["id"] = json!(id);
                profiles.entry(id).or_insert(profile);
            }
        }
        Ok(profiles.into_values().collect())
    }

    pub(super) fn translation_identities(&self, project: &str, recording: &str) -> Result<Value> {
        let source = self.recording_file(project, recording, false, false)?;
        let cache = contained_path(
            &source.folder,
            &format!("indexes/{recording}/translation-topics.json"),
        )?;
        if let Ok(saved) = read_json(&cache) {
            if saved["sha256"] == source.item["sha256"] {
                return Ok(saved["topics"].clone());
            }
        }
        let topics = crate::engine::translation_channels(&source.path)?;
        crate::storage::write_changed(
            &cache,
            &json!({"sha256":source.item["sha256"],"topics":topics}),
        )?;
        Ok(topics)
    }

    pub fn topic_translations(&self, project: &str, recording: &str) -> Result<Value> {
        let profiles = self.translation_library(project)?;
        let mut topics = self.translation_identities(project, recording)?;
        for topic in topics
            .as_array_mut()
            .context("Invalid translation topics")?
        {
            topic["choices"] = json!(profiles
                .iter()
                .filter(|profile| matches(profile, topic))
                .collect::<Vec<_>>());
        }
        Ok(json!({"topics":topics}))
    }

    fn translation_topic(&self, project: &str, recording: &str, channel: &str) -> Result<Value> {
        self.translation_identities(project, recording)?
            .as_array()
            .context("Missing topics")?
            .iter()
            .find(|topic| topic["channel_id"] == channel)
            .cloned()
            .context("Translation topic not found")
    }

    pub fn preview_topic_translation(
        &self,
        project: &str,
        recording: &str,
        request: &Value,
    ) -> Result<Value> {
        let channel = field(request, "channel_id")?;
        self.translation_topic(project, recording, channel)?;
        let source = self.recording_file(project, recording, false, false)?;
        let sample = crate::engine::translation_sample(&source.path, channel.parse()?)?;
        let script = field(request, "script")?;
        ensure!(
            script.len() <= 256 * 1024,
            "Translation script is too large"
        );
        let translated = crate::engine::translate_record(&sample, script)?;
        Ok(json!({"source":preview_value(&sample),"translated":preview_value(&translated)}))
    }

    pub fn save_topic_translation(
        &self,
        project: &str,
        recording: &str,
        request: &Value,
        actor: &crate::accounts::Principal,
    ) -> Result<Value> {
        let channel = field(request, "channel_id")?;
        let topic = self.translation_topic(project, recording, channel)?;
        let mut profile = request["translation"].clone();
        ensure!(profile.is_object(), "Provide a translation file");
        ensure!(profile["output_schema"].is_null(), "This translation requires Protobuf output; native topic translations currently produce JSON records");
        if let Some(output_type) = profile.get("output_type") {
            ensure!(
                output_type
                    .as_str()
                    .is_some_and(|name| !name.trim().is_empty()
                        && name.len() <= 200
                        && !name.chars().any(char::is_control)),
                "Translation output_type must be a nonempty message type"
            );
        }
        ensure!(
            profile.get("format_version").is_none_or(|v| v == 1),
            "Unsupported translation file version"
        );
        for key in ["topic", "source_message", "source_schema_sha256"] {
            ensure!(
                profile.get(key).is_none_or(|v| v == &topic[key]),
                "Translation belongs to another topic, message type, or schema version"
            );
            profile[key] = topic[key].clone();
        }
        let name = valid_name(&profile)?;
        let script = crate::engine::translation_file_script(&profile)?;
        ensure!(
            !script.trim().is_empty(),
            "Translation script must contain mappings"
        );
        self.preview_topic_translation(
            project,
            recording,
            &json!({"channel_id":channel,"script":script}),
        )?;
        let id = format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{}\n{}\n{}\n{name}",
                    topic["topic"], topic["source_message"], topic["source_schema_sha256"]
                )
                .as_bytes()
            )
        );
        profile["id"] = json!(id);
        profile["format_version"] = json!(1);
        profile["name"] = json!(name);
        profile["script"] = json!(script);
        profile["output_type"] = profile
            .get("output_type")
            .cloned()
            .unwrap_or(json!("TrainingRecord"));
        profile["updated_at"] = json!(now());
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(project)?;
        let _lock = Self::lock_project(&folder)?;
        self.accounts
            .require_project(actor, &folder, "project.manage")?;
        Self::editable(&self.cached_manifest(&folder)?)?;
        crate::storage::write_changed(
            &contained_path(&folder, &format!("translations/library/{id}.json"))?,
            &profile,
        )?;
        Ok(profile)
    }

    pub fn apply_topic_translations(
        &self,
        project: &str,
        recording: &str,
        request: &Value,
    ) -> Result<Value> {
        let profiles = self.translation_library(project)?;
        let selection = request["selections"]
            .as_object()
            .context("Choose translations for the topics")?;
        ensure!(
            !selection.is_empty(),
            "Choose at least one topic translation"
        );
        let mut scripts = serde_json::Map::new();
        for (channel, id) in selection {
            let topic = self.translation_topic(project, recording, channel)?;
            let profile = profiles
                .iter()
                .find(|profile| profile["id"] == *id && matches(profile, &topic))
                .context("Translation does not match this topic and schema version")?;
            ensure!(profile["output_schema"].is_null(), "This translation requires Protobuf output; native topic translations currently produce JSON records");
            scripts.insert(
                channel.clone(),
                json!(crate::engine::translation_file_script(profile)?),
            );
        }
        let channels: Vec<_> = self
            .translation_identities(project, recording)?
            .as_array()
            .context("Missing topics")?
            .iter()
            .map(|topic| topic["channel_id"].clone())
            .collect();
        self.pipeline(project,&json!({"recording_uuid":recording,"selected_channels":channels,"translations":scripts,"preserve_untranslated":true,"sync":{"enabled":false}}))
    }

    /// Apply only unambiguous, natively supported presets to a newly uploaded recording.
    /// Import remains usable if a preset fails against any later message.
    pub fn auto_translate_import(&self, project: &str, imported: Value) -> Value {
        let result = (|| -> Result<Value> {
            let profiles = self.translation_library(project)?;
            if profiles.is_empty() {
                return Ok(imported.clone());
            }
            let recording = field(&imported, "uuid")?;
            let topics = self.translation_identities(project, recording)?;
            let mut selections = Map::new();
            let mut ambiguous = Vec::new();
            for topic in topics.as_array().context("Missing translation topics")? {
                let matching: Vec<_> = profiles
                    .iter()
                    .filter(|profile| matches(profile, topic))
                    .collect();
                if matching.len() == 1 && matching[0]["output_schema"].is_null() {
                    selections.insert(
                        field(topic, "channel_id")?.to_owned(),
                        matching[0]["id"].clone(),
                    );
                } else if !matching.is_empty() {
                    ambiguous.push(field(topic, "topic")?.to_owned());
                }
            }
            let mut output = if selections.is_empty() {
                imported.clone()
            } else {
                let mut output = self.apply_topic_translations(
                    project,
                    recording,
                    &json!({"selections":selections}),
                )?;
                output["source_recording"] = imported.clone();
                output["translated_topics"] = json!(selections.len());
                output
            };
            if !ambiguous.is_empty() {
                output["translation_warning"] = json!(format!("Choose a translation manually for {}: multiple presets or a preset requiring Protobuf output.",ambiguous.join(", ")));
            }
            Ok(output)
        })();
        result.unwrap_or_else(|error| {
            let mut output = imported;
            output["translation_warning"] = json!(format!(
                "MCAP imported; automatic translations could not be applied: {error:#}"
            ));
            output
        })
    }
}
