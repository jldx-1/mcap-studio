//! Versioned project descriptors, metadata references and incremental publication.
use super::*;

pub(crate) fn metadata_path(kind: &str, id: &str) -> String {
    match kind {
        "mcaps" => format!("mcap/{id}/metadata.json"),
        "labels" => format!("labels/{id}/label.json"),
        "label_lists" => format!("labels/lists/{id}.json"),
        "datasets" => format!("datasets/{id}/dataset.json"),
        _ => unreachable!(),
    }
}

pub fn resolve_manifest(folder: &Path) -> Result<Value> {
    let mut manifest = read_json(&contained_path(folder, "project.json")?)?;
    if manifest["format_version"] != 3 {
        return Ok(manifest);
    }
    for kind in REGISTRIES {
        let refs = manifest[kind]
            .as_object()
            .context("Registry must be an object")?;
        let mut items = Map::new();
        for (id, reference) in refs {
            let expected = metadata_path(kind, id);
            ensure!(
                reference["metadata"] == expected,
                "Registry metadata path does not match entity"
            );
            let item = read_json(&contained_path(folder, &expected)?)?;
            ensure!(
                item["uuid"] == id.as_str(),
                "Metadata UUID does not match registry"
            );
            items.insert(id.clone(), item);
        }
        manifest[kind] = json!(items);
    }
    Ok(manifest)
}

/// Small metadata files are the only dependencies of the project listing.
pub fn manifest_stamp(folder: &Path) -> Result<Value> {
    let raw = read_json(&contained_path(folder, "project.json")?)?;
    let mut stamps = json!({"project":fingerprint(&contained_path(folder, "project.json")?)?});
    if raw["format_version"] == 3 {
        for kind in REGISTRIES {
            for (id, reference) in raw[kind].as_object().context("Invalid registry")? {
                let relative = reference["metadata"]
                    .as_str()
                    .context("Missing metadata path")?;
                ensure!(
                    relative == metadata_path(kind, id),
                    "Registry metadata path does not match entity"
                );
                stamps[format!("{kind}:{id}")] = fingerprint(&contained_path(folder, relative)?)?;
            }
        }
    }
    Ok(stamps)
}

/// Reuse the already validated references while the root manifest is unchanged.
pub(crate) fn known_manifest_stamp(
    folder: &Path,
    saved: &Value,
    manifest: &Value,
) -> Result<Value> {
    let root = fingerprint(&contained_path(folder, "project.json")?)?;
    if root != saved["project"] {
        return manifest_stamp(folder);
    }
    let mut stamps = json!({"project":root});
    if manifest["format_version"] == 3 {
        for kind in REGISTRIES {
            for id in manifest[kind]
                .as_object()
                .context("Invalid registry")?
                .keys()
            {
                stamps[format!("{kind}:{id}")] =
                    fingerprint(&contained_path(folder, &metadata_path(kind, id))?)?;
            }
        }
    }
    Ok(stamps)
}

pub fn save_manifest(folder: &Path, manifest: &Value) -> Result<()> {
    if manifest["format_version"] != 3 {
        return write_changed(&contained_path(folder, "project.json")?, manifest);
    }
    let mut root = manifest.clone();
    for kind in REGISTRIES {
        let mut refs = Map::new();
        for (id, item) in manifest[kind].as_object().context("Invalid registry")? {
            let metadata = if kind == "datasets" {
                dataset_metadata(folder, item)?
            } else {
                item.clone()
            };
            let relative = metadata_path(kind, id);
            write_changed(&contained_path(folder, &relative)?, &metadata)?;
            refs.insert(id.clone(), json!({"metadata":relative}));
        }
        root[kind] = json!(refs);
    }
    write_changed(&contained_path(folder, "project.json")?, &root)
}

/// A mutation already knows its previous snapshot; compare in memory, without
/// opening every unchanged entity file on a Windows-mounted filesystem.
pub(crate) fn save_manifest_delta(folder: &Path, manifest: &Value, previous: &Value) -> Result<()> {
    if manifest["format_version"] != 3 {
        return save_manifest(folder, manifest);
    }
    let mut root = manifest.clone();
    let mut old_root = previous.clone();
    for kind in REGISTRIES {
        let mut refs = Map::new();
        let mut old_refs = Map::new();
        for (id, item) in manifest[kind].as_object().context("Invalid registry")? {
            let relative = metadata_path(kind, id);
            if previous[kind].get(id) != Some(item) {
                let metadata = if kind == "datasets" {
                    dataset_metadata(folder, item)?
                } else {
                    item.clone()
                };
                write_json(&contained_path(folder, &relative)?, &metadata)?;
            }
            refs.insert(id.clone(), json!({"metadata":relative}));
        }
        for id in previous[kind]
            .as_object()
            .context("Invalid registry")?
            .keys()
        {
            old_refs.insert(id.clone(), json!({"metadata":metadata_path(kind,id)}));
        }
        root[kind] = json!(refs);
        old_root[kind] = json!(old_refs);
    }
    if root != old_root {
        write_json(&contained_path(folder, "project.json")?, &root)?;
    }
    Ok(())
}
