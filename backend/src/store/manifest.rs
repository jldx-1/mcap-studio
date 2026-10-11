//! Validation of supported project formats and registry provenance.
use anyhow::{ensure, Context, Result};
use serde_json::{Map, Value};
use std::{collections::HashSet, path::Path};

use super::{canonical_uuid, contained_path, field, registry, valid_name};

pub(super) fn load_manifest(folder: &Path) -> Result<Value> {
    let value = crate::storage::resolve_manifest(folder)?;
    ensure!(
        value["kind"] == "mcap-labeling-project"
            && matches!(value["format_version"].as_u64(), Some(1..=3)),
        "Unsupported MCAP project"
    );
    canonical_uuid(field(&value, "uuid")?)?;
    if value["format_version"] == 1 {
        let path = contained_path(folder, field(&value, "mcap")?)?;
        ensure!(
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("mcap")),
            "Project recording must be an MCAP file"
        );
        return Ok(value);
    }
    registry(&value, "tasks")?;
    for name in ["mcaps", "labels"] {
        for (id, item) in registry(&value, name)? {
            canonical_uuid(id)?;
            ensure!(item["uuid"] == id.as_str(), "Mismatched UUID in {name}");
            let path = contained_path(folder, field(item, "path")?)?;
            let expected = folder
                .join(if name == "mcaps" { "mcap" } else { "labels" })
                .join(id);
            ensure!(
                path.is_file() && path.parent() == Some(expected.as_path()),
                "Registry files must be inside their UUID folder"
            );
            for key in [
                "configuration",
                "workflow",
                "translation",
                "synchronization",
                "frame_map",
                "annotations",
            ] {
                if let Some(relative) = item[key].as_str() {
                    contained_path(folder, relative)?;
                }
            }
            if name == "mcaps" {
                ensure!(
                    path.extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("mcap")),
                    "Recording must reference MCAP data"
                );
            }
            for label in item["label_ids"].as_array().into_iter().flatten() {
                ensure!(
                    value["labels"]
                        .get(label.as_str().context("Label UUID must be a string")?)
                        .is_some(),
                    "Unknown project label UUID"
                );
            }
        }
    }
    for name in ["label_lists", "datasets"] {
        if let Some(items) = value.get(name) {
            let items = items.as_object().context("Registry must be an object")?;
            let mut names = HashSet::new();
            for (id, item) in items {
                canonical_uuid(id)?;
                ensure!(item["uuid"] == id.as_str(), "Mismatched UUID in {name}");
                ensure!(
                    names.insert(valid_name(item)?),
                    "Names must be unique in {name}"
                );
                if name == "label_lists" {
                    for label in item["label_ids"]
                        .as_array()
                        .context("Label list needs label_ids")?
                    {
                        ensure!(
                            value["labels"]
                                .get(label.as_str().context("Label UUID must be a string")?)
                                .is_some(),
                            "Unknown project label UUID"
                        );
                    }
                } else {
                    if item["format"] == "csv" {
                        ensure!(
                            contained_path(folder, field(item, "csv_path")?)?
                                == contained_path(folder, &format!("datasets/{id}/data.csv"))?,
                            "CSV path does not match dataset"
                        );
                        ensure!(item["columns"].is_array(), "CSV dataset needs columns");
                    }
                    let empty = Map::new();
                    let entries = if value["format_version"] == 3 {
                        let pages = item["entry_pages"]
                            .as_object()
                            .context("Dataset needs entry pages")?;
                        let mut count = 0u64;
                        for page in pages.values() {
                            let rows = page["count"].as_u64().context("Page needs a count")?;
                            ensure!(
                                rows <= crate::storage::PAGE_SIZE,
                                "Dataset page exceeds its size limit"
                            );
                            count = count.checked_add(rows).context("Dataset count overflow")?;
                        }
                        ensure!(
                            item["entry_count"] == count,
                            "Dataset entry count does not match pages"
                        );
                        &empty
                    } else {
                        item["entries"]
                            .as_object()
                            .context("Dataset entries must be an object")?
                    };
                    for (key, entry) in entries {
                        let recording_id = field(entry, "mcap_uuid")?;
                        let recording = value["mcaps"]
                            .get(recording_id)
                            .context("Dataset references unknown recording")?;
                        let channel = field(entry, "channel_id")?;
                        ensure!(
                            channel.parse::<u16>().is_ok(),
                            "Dataset channel must be numeric"
                        );
                        let number = entry["frame"]
                            .as_u64()
                            .context("Dataset frame must be a nonnegative integer")?;
                        ensure!(
                            key == &format!("{recording_id}:{channel}:{number}"),
                            "Dataset key does not match frame"
                        );
                        let raw = &entry["raw_data"];
                        ensure!(
                            raw["mcap"] == recording["path"]
                                && raw["sha256"] == recording["sha256"],
                            "Dataset provenance does not match recording"
                        );
                        if item["format"] == "csv" {
                            ensure!(entry["values"].is_object(), "CSV entry needs column values");
                            continue;
                        }
                        let recording_dir = contained_path(folder, field(recording, "path")?)?
                            .parent()
                            .unwrap()
                            .to_owned();
                        ensure!(
                            contained_path(folder, field(raw, "frame_index")?)?
                                == recording_dir.join("frames/index.json"),
                            "Dataset frame index does not match recording"
                        );
                        ensure!(
                            contained_path(folder, field(&entry["labeled_data"], "path")?)?
                                == recording_dir.join("annotations.json"),
                            "Dataset annotations do not match recording"
                        );
                        for name in ["image", "records"] {
                            if let Some(relative) = raw[name].as_str() {
                                ensure!(
                                    contained_path(folder, relative)?
                                        .starts_with(recording_dir.join("frames")),
                                    "Dataset raw data escapes frame cache"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(value)
}
