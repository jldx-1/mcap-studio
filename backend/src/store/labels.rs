//! Named entity registration and shared label/list management.
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;

use super::{canonical_uuid, contained_path, field, registry, valid_name, write_json, Store};

impl Store {
    pub fn add_named(&self, id: &str, kind: &str, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let (folder, _) = self.manifest(id)?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = self.cached_manifest(&folder)?;
        let previous = manifest.clone();
        Self::editable(&manifest)?;
        let name = valid_name(request)?;
        ensure!(
            !manifest[kind]
                .as_object()
                .into_iter()
                .flat_map(|items| items.values())
                .any(|item| item["name"] == name),
            "A {kind} entry with this name already exists"
        );
        let uuid = Uuid::new_v4().to_string();
        let item = match kind {
            "labels" => {
                json!({"uuid":uuid,"name":name,"type":"any","path":format!("labels/{uuid}/label.json")})
            }
            "label_lists" => {
                let label_ids = request["label_ids"]
                    .as_array()
                    .context("Provide label_ids")?;
                let mut seen = HashSet::new();
                for label in label_ids {
                    let label = label.as_str().context("Label UUID must be a string")?;
                    ensure!(
                        manifest["labels"].get(label).is_some() && seen.insert(label),
                        "Label lists require unique existing labels"
                    );
                }
                json!({"uuid":uuid,"name":name,"label_ids":label_ids})
            }
            "datasets" => json!({"uuid":uuid,"name":name,"format":"yolo","entries":{}}),
            _ => bail!("Unknown registry"),
        };
        if !manifest[kind].is_object() {
            manifest[kind] = json!({});
        }
        manifest[kind][&uuid] = item.clone();
        if kind == "labels" {
            if manifest["format_version"] == 2 {
                write_json(&contained_path(&folder, field(&item, "path")?)?, &item)?;
            }
            let ids: Vec<Value> = registry(&manifest, "labels")?
                .keys()
                .map(|id| json!(id))
                .collect();
            if manifest["format_version"] == 2 {
                for item in manifest["mcaps"].as_object_mut().unwrap().values_mut() {
                    item["label_ids"] = json!(ids);
                }
            }
        }
        crate::storage::save_manifest_delta(&folder, &manifest, &previous)?;
        Ok(self.detail(&folder, &manifest))
    }

    pub fn add_list_label(&self, id: &str, list_id: &str, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = self.cached_manifest(&folder)?;
        let previous = manifest.clone();
        Self::editable(&manifest)?;
        canonical_uuid(list_id)?;
        ensure!(
            manifest["label_lists"].get(list_id).is_some(),
            "Label list not found"
        );
        let name = valid_name(request)?;
        let existing = registry(&manifest, "labels")?
            .iter()
            .find(|(_, label)| label["name"] == name)
            .map(|(id, _)| id.clone());
        let uuid = existing.unwrap_or_else(|| Uuid::new_v4().to_string());
        let ids = manifest["label_lists"][list_id]["label_ids"]
            .as_array_mut()
            .context("Invalid label list")?;
        ensure!(
            !ids.contains(&json!(uuid)),
            "Label already exists in this list"
        );
        ids.push(json!(uuid));
        if manifest["labels"].get(&uuid).is_none() {
            let item = json!({"uuid":uuid,"name":name,"type":"any","path":format!("labels/{uuid}/label.json")});
            if manifest["format_version"] == 2 {
                write_json(&contained_path(&folder, field(&item, "path")?)?, &item)?;
            }
            manifest["labels"][&uuid] = item;
        }
        let all_ids: Vec<_> = registry(&manifest, "labels")?
            .keys()
            .map(|id| json!(id))
            .collect();
        if manifest["format_version"] == 2 {
            for item in manifest["mcaps"].as_object_mut().unwrap().values_mut() {
                item["label_ids"] = json!(all_ids);
            }
        }
        crate::storage::save_manifest_delta(&folder, &manifest, &previous)?;
        Ok(self.detail(&folder, &manifest))
    }

    pub fn update_label_list(&self, id: &str, list_id: &str, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = self.cached_manifest(&folder)?;
        let previous_manifest = manifest.clone();
        Self::editable(&manifest)?;
        canonical_uuid(list_id)?;
        let list = manifest["label_lists"]
            .get(list_id)
            .context("Label list not found")?;
        let previous = list["label_ids"]
            .as_array()
            .context("Invalid label list")?
            .clone();
        if let Some(expected) = request.get("expected_label_ids") {
            ensure!(
                *expected == json!(previous),
                "Label list changed; refresh before editing its order"
            );
        }
        let ids = request["label_ids"]
            .as_array()
            .context("Provide ordered label_ids")?;
        let mut seen = HashSet::new();
        for label in ids {
            let uuid = label.as_str().context("Label UUID must be a string")?;
            ensure!(
                manifest["labels"].get(uuid).is_some() && seen.insert(uuid),
                "Label lists require unique existing labels"
            );
        }
        // Keep the stable class identities of saved annotations after list membership changes.
        let mut retired = list["retired_label_ids"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for label in previous {
            if !ids.contains(&label) && !retired.contains(&label) {
                retired.push(label);
            }
        }
        retired.retain(|label| !ids.contains(label));
        manifest["label_lists"][list_id]["label_ids"] = json!(ids);
        manifest["label_lists"][list_id]["retired_label_ids"] = json!(retired);
        crate::storage::save_manifest_delta(&folder, &manifest, &previous_manifest)?;
        Ok(self.detail(&folder, &manifest))
    }

    pub fn update_label(&self, id: &str, label_id: &str, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let mut manifest = self.cached_manifest(&folder)?;
        let previous = manifest.clone();
        Self::editable(&manifest)?;
        let label = manifest["labels"]
            .get(label_id)
            .context("Label not found")?;
        if let Some(expected) = request.get("expected_name") {
            ensure!(
                label["name"] == *expected,
                "Label changed; refresh before saving"
            );
        }
        let name = valid_name(request)?;
        ensure!(
            !registry(&manifest, "labels")?
                .iter()
                .any(|(uuid, label)| uuid != label_id && label["name"] == name),
            "Label name already exists"
        );
        manifest["labels"][label_id]["name"] = json!(name);
        if manifest["format_version"] == 2 {
            crate::storage::write_changed(
                &contained_path(&folder, field(&manifest["labels"][label_id], "path")?)?,
                &manifest["labels"][label_id],
            )?;
        }
        crate::storage::save_manifest_delta(&folder, &manifest, &previous)?;
        Ok(self.detail(&folder, &manifest))
    }
}
