//! Application-facing project handle. Directory details belong to Store.
use crate::Store;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

pub struct Project {
    store: Store,
    uuid: String,
}

impl Project {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let folder = if path.is_file() {
            path.parent().context("Missing project folder")?
        } else {
            path
        };
        let store = Store::new(folder)?;
        let opened = crate::storage::resolve_manifest(folder)?;
        let uuid = opened["uuid"]
            .as_str()
            .context("Missing project UUID")?
            .to_owned();
        store.project(&uuid)?;
        Ok(Self { store, uuid })
    }

    pub fn metadata(&self) -> Result<Value> {
        self.store.project(&self.uuid)
    }
    pub fn mcaps(&self) -> Result<Value> {
        Ok(self.metadata()?["mcaps"].clone())
    }
    pub fn labels(&self) -> Result<Value> {
        Ok(self.metadata()?["labels"].clone())
    }
    pub fn datasets(&self) -> Result<Value> {
        Ok(self.metadata()?["datasets"].clone())
    }
    pub fn annotations(&self, recording: &str) -> Result<Value> {
        Ok(self.store.recording(&self.uuid, recording)?["annotations"].clone())
    }
    pub fn update_annotations(&self, recording: &str, patch: &Value) -> Result<Value> {
        self.store.patch_annotations(&self.uuid, recording, patch)
    }
    pub fn add_annotation(
        &self,
        recording: &str,
        channel: &str,
        frame: u64,
        annotation: &Value,
    ) -> Result<Value> {
        let sha256 = self.store.recording(&self.uuid, recording)?["sha256"].clone();
        self.update_annotations(recording, &json!({"sha256":sha256,"changes":[{"channel_id":channel,"frame":frame,"annotation":annotation,"expected":null}]}))
    }
    pub fn update_label(&self, label: &str, request: &Value) -> Result<Value> {
        self.store.update_label(&self.uuid, label, request)
    }
    pub fn create_dataset(&self, name: &str) -> Result<Value> {
        self.store
            .add_named(&self.uuid, "datasets", &json!({"name":name}))
    }
    pub fn dataset(&self, name_or_uuid: &str) -> Result<Value> {
        let items = self.datasets()?;
        let item = items
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["uuid"] == name_or_uuid || item["name"] == name_or_uuid)
            .context("Dataset not found")?;
        self.store
            .dataset(&self.uuid, item["uuid"].as_str().unwrap())
    }
    pub fn build_dataset(&self, uuid: &str, format: &str) -> Result<Value> {
        self.store
            .build_dataset(&self.uuid, uuid, &json!({"format":format}))
    }
}
