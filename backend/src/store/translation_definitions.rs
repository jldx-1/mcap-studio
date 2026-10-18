//! Uploaded Protobuf input definitions and reusable Protobuf-to-CSV mappings.
use super::{contained_path, field, now, read_json, valid_name, Store};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path},
    process::Command,
};

fn describe(id: &str, bytes: &[u8]) -> Result<Vec<Value>> {
    let pool = DescriptorPool::decode(bytes)?;
    Ok(pool.all_messages().filter(|message|!message.is_map_entry()&&!message.full_name().starts_with("google.protobuf.")).map(|message| {
        let fields:Vec<_>=message.fields().map(|field|json!({"name":field.name(),"type":format!("{:?}",field.kind()),"repeated":field.is_list()})).collect();
        json!({"id":id,"message":message.full_name(),"encoding":"protobuf","fields":fields})
    }).collect())
}
impl Store {
    pub fn translation_definitions(&self, project: &str) -> Result<Value> {
        let folder = self.folder(project)?;
        let root = contained_path(&folder, "translations/definitions")?;
        let mut definitions = Vec::new();
        if root.is_dir() {
            for entry in fs::read_dir(root)? {
                let entry = entry?;
                if !entry.file_type()?.is_file()
                    || entry.path().extension().is_none_or(|ext| ext != "json")
                {
                    continue;
                }
                let path = contained_path(
                    &folder,
                    &format!(
                        "translations/definitions/{}",
                        entry.file_name().to_string_lossy()
                    ),
                )?;
                let saved = read_json(&path)?;
                let bytes = STANDARD.decode(field(&saved, "descriptor_base64")?)?;
                definitions.extend(describe(field(&saved, "id")?, &bytes)?);
            }
        }
        Ok(json!({"definitions":definitions}))
    }
    pub fn upload_translation_definitions(
        &self,
        project: &str,
        request: &Value,
        actor: &crate::accounts::Principal,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        self.accounts
            .require_project(actor, &folder, "project.manage")?;
        let files = request["files"]
            .as_array()
            .context("Choose Protobuf definition files")?;
        ensure!(
            !files.is_empty() && files.len() <= 64,
            "Choose 1–64 definition files"
        );
        let temporary = tempfile::tempdir()?;
        let mut protos = Vec::new();
        let mut descriptors = prost_types::FileDescriptorSet::default();
        let mut total = 0;
        let mut names = std::collections::HashSet::new();
        for file in files {
            let name = field(file, "name")?;
            ensure!(
                !name.is_empty() && names.insert(name),
                "Definition filenames must be unique and nonempty"
            );
            ensure!(
                !name.contains('\\')
                    && Path::new(name)
                        .components()
                        .all(|part| matches!(part, Component::Normal(_))),
                "Definition filenames must be relative paths without .."
            );
            let bytes = STANDARD
                .decode(field(file, "data_base64")?)
                .context("Invalid definition file bytes")?;
            total += bytes.len();
            ensure!(
                total <= 8 * 1024 * 1024,
                "Definitions must total at most 8 MiB"
            );
            if name.ends_with(".proto") {
                let path = temporary.path().join(name);
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(&path, bytes)?;
                protos.push(path);
            } else {
                descriptors.file.extend(
                    prost_types::FileDescriptorSet::decode(bytes.as_slice())
                        .context(
                            "Expected a .proto file or binary FileDescriptorSet (.desc/.pb/.bin)",
                        )?
                        .file,
                );
            }
        }
        if !protos.is_empty() {
            let output = temporary.path().join("compiled.desc");
            let result=Command::new("protoc").arg(format!("--proto_path={}",temporary.path().display())).arg("--include_imports").arg(format!("--descriptor_set_out={}",output.display())).args(protos).output().context("Loading .proto files requires protoc on the server. Alternatively upload a compiled FileDescriptorSet (.desc) including its imports")?;
            ensure!(
                result.status.success(),
                "Invalid Protobuf definitions: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            descriptors
                .file
                .extend(prost_types::FileDescriptorSet::decode(fs::read(output)?.as_slice())?.file);
        }
        descriptors.file.sort_by(|a, b| a.name.cmp(&b.name));
        for files in descriptors.file.windows(2) {
            ensure!(
                files[0].name != files[1].name || files[0] == files[1],
                "Conflicting Protobuf files with the same name"
            );
        }
        descriptors.file.dedup_by(|a, b| a.name == b.name);
        let bytes = descriptors.encode_to_vec();
        let id = format!("{:x}", Sha256::digest(&bytes));
        let definitions = describe(&id, &bytes)?;
        ensure!(
            !definitions.is_empty(),
            "No input message types found in these definitions"
        );
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        Self::editable(&self.cached_manifest(&folder)?)?;
        crate::storage::write_changed(
            &contained_path(&folder, &format!("translations/definitions/{id}.json"))?,
            &json!({"id":id,"descriptor_base64":STANDARD.encode(bytes)}),
        )?;
        Ok(json!({"definitions":definitions}))
    }
    fn definition_source(&self, project: &str, request: &Value) -> Result<(Value, Value)> {
        let id = field(request, "definition_id")?;
        ensure!(
            id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid definition ID"
        );
        let folder = self.folder(project)?;
        let saved = read_json(&contained_path(
            &folder,
            &format!("translations/definitions/{id}.json"),
        )?)?;
        let bytes = STANDARD.decode(field(&saved, "descriptor_base64")?)?;
        let pool = DescriptorPool::decode(bytes.as_slice())?;
        let name = field(request, "message")?;
        let descriptor = pool
            .get_message_by_name(name)
            .context("Message type not found in definition")?;
        let sample = if let Some(encoded) = request["sample_base64"].as_str() {
            let bytes = STANDARD.decode(encoded)?;
            ensure!(
                bytes.len() <= 8 * 1024 * 1024,
                "Sample must be at most 8 MiB"
            );
            DynamicMessage::decode(descriptor, bytes.as_slice())?
        } else if request["sample"].is_null() {
            DynamicMessage::new(descriptor)
        } else {
            let sample = serde_json::to_string(&request["sample"])?;
            DynamicMessage::deserialize(
                descriptor,
                &mut serde_json::Deserializer::from_str(&sample),
            )
            .context("Sample does not match this Protobuf message type")?
        };
        let source = crate::engine::protobuf_json(&sample)?;
        // Use the same canonical descriptor fingerprint as embedded MCAP types.
        let mut set = prost_types::FileDescriptorSet::decode(bytes.as_slice())?;
        set.file.sort_by(|a, b| a.name.cmp(&b.name));
        let fingerprint = format!("{:x}", Sha256::digest(set.encode_to_vec()));
        Ok((
            source,
            json!({"source_message":name,"source_schema_sha256":fingerprint}),
        ))
    }
    pub fn preview_definition_translation(&self, project: &str, request: &Value) -> Result<Value> {
        let (source, _) = self.definition_source(project, request)?;
        let script = field(request, "script")?;
        ensure!(script.len() <= 256 * 1024, "Script must be at most 256 KiB");
        let translated = crate::engine::translate_record(&source, script)?;
        let mut columns = serde_json::Map::new();
        super::structured::flatten_message(&translated, "", &mut columns);
        Ok(
            json!({"source":source,"translated":translated,"columns":columns.keys().collect::<Vec<_>>(),"row":columns}),
        )
    }
    pub fn save_definition_translation(
        &self,
        project: &str,
        request: &Value,
        actor: &crate::accounts::Principal,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        self.accounts
            .require_project(actor, &folder, "project.manage")?;
        self.preview_definition_translation(project, request)?;
        let (_, mut profile) = self.definition_source(project, request)?;
        let name = valid_name(request)?;
        let script = field(request, "script")?;
        ensure!(!script.trim().is_empty(), "Provide a translation script");
        let id = format!(
            "{:x}",
            Sha256::digest(format!(
                "{}\n{}\n{name}",
                profile["source_message"], profile["source_schema_sha256"]
            ))
        );
        profile["id"] = json!(id);
        profile["name"] = json!(name);
        profile["format_version"] = json!(1);
        profile["script"] = json!(script);
        profile["output_type"] = json!("TrainingRecord");
        profile["output_format"] = json!("csv");
        profile["updated_at"] = json!(now());
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let _lock = Self::lock_project(&folder)?;
        Self::editable(&self.cached_manifest(&folder)?)?;
        crate::storage::write_changed(
            &contained_path(&folder, &format!("translations/library/{id}.json"))?,
            &profile,
        )?;
        Ok(profile)
    }
}
