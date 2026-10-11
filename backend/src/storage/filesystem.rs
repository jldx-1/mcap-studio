//! Atomic file access, containment, fingerprints and canonical JSON hashes.
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
};

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .context("JSON file needs a parent directory")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer = BufWriter::new(&mut temporary);
        serde_json::to_writer_pretty(&mut writer, value)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn file_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Reject traversal and resolve existing components to catch symlinks and junctions.
pub fn contained_path(base: &Path, relative: &str) -> Result<PathBuf> {
    let base = base.canonicalize()?;
    let relative = Path::new(relative);
    ensure!(
        !relative.as_os_str().is_empty() && !relative.is_absolute(),
        "Project paths must be relative"
    );
    ensure!(
        relative
            .components()
            .all(|part| matches!(part, Component::Normal(_))),
        "Project paths must stay inside their folder"
    );
    // Existing files need one complete resolution, rather than resolving every
    // prefix separately. This matters on Windows-mounted project directories.
    if let Ok(path) = base.join(relative).canonicalize() {
        ensure!(
            path != base && path.starts_with(&base),
            "Project path escapes its folder"
        );
        return Ok(path);
    }
    let mut path = base.clone();
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                path.push(part);
                if path.symlink_metadata().is_ok() {
                    path = path.canonicalize()?;
                }
                ensure!(path.starts_with(&base), "Project path escapes its folder");
            }
            _ => bail!("Project paths must stay inside their folder"),
        }
    }
    ensure!(
        path != base && path.starts_with(&base),
        "Project path escapes its folder"
    );
    Ok(path)
}

pub(crate) fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?,
    )
    .context("Invalid JSON file")
}

pub(crate) fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        ensure!(
            !entry.file_type()?.is_symlink(),
            "Frame cache cannot contain symlinks"
        );
        let destination = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

pub(crate) fn list_files(directory: &Path) -> Result<Vec<String>> {
    fn visit(base: &Path, directory: &Path, output: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            ensure!(
                !entry.file_type()?.is_symlink(),
                "Exports cannot contain symlinks"
            );
            if entry.file_type()?.is_dir() {
                visit(base, &entry.path(), output)?;
            } else {
                output.push(
                    entry
                        .path()
                        .strip_prefix(base)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(directory, directory, &mut files)?;
    files.sort();
    Ok(files)
}

pub fn digest(value: &Value) -> Result<String> {
    // Sorting recursively makes fingerprints independent of JSON key order.
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(items) => {
                let items: BTreeMap<_, _> = items.iter().map(|(k, v)| (k, sorted(v))).collect();
                serde_json::to_value(items).unwrap()
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            _ => value.clone(),
        }
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&sorted(value))?)
    ))
}

/// Size, nanosecond mtime, and file identity; Unix ctime catches restored mtimes.
pub fn fingerprint(path: &Path) -> Result<Value> {
    let metadata = path.metadata()?;
    let mut stamp = json!({"size":metadata.len(), "modified_ns":metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_nanos().to_string()});
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        stamp["device"] = json!(metadata.dev());
        stamp["inode"] = json!(metadata.ino());
        stamp["ctime"] = json!([metadata.ctime(), metadata.ctime_nsec()]);
    }
    #[cfg(not(unix))]
    {
        stamp["created"] = json!(metadata
            .created()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|time| time.as_nanos().to_string()));
    }
    Ok(stamp)
}

pub fn write_changed(path: &Path, value: &Value) -> Result<()> {
    if path.is_file() && read_json(path).ok().as_ref() == Some(value) {
        return Ok(());
    }
    write_json(path, value)
}
