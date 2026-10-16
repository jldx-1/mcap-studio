//! Imported YOLO detection datasets own their images and normalized annotations.
use super::{canonical_uuid, contained_path, field, now, valid_name, Store};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(crate) fn upload_name(name: &str) -> Result<String> {
    let name = name.replace('\\', "/");
    let name = name.trim_start_matches("./").to_owned();
    ensure!(
        !name.is_empty()
            && !name.starts_with('/')
            && !name.contains(':')
            && !name.chars().any(char::is_control)
            && name
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "Dataset files must have relative paths without parent traversal"
    );
    Ok(name)
}

fn pairing_key(name: &str) -> String {
    let path = Path::new(name).with_extension("");
    path.to_string_lossy()
        .split('/')
        .filter(|part| !matches!(*part, "images" | "labels"))
        .collect::<Vec<_>>()
        .join("/")
}

fn class_names(files: &[(String, PathBuf)], supplied: &[String]) -> Result<Vec<String>> {
    if !supplied.is_empty() {
        return validate_names(supplied.to_vec());
    }
    let mut candidates = Vec::new();
    for (name, path) in files {
        let filename = Path::new(name)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        if matches!(filename.as_str(), "classes.txt" | "obj.names") {
            candidates.push(
                fs::read_to_string(path)?
                    .lines()
                    .map(str::to_owned)
                    .collect(),
            );
        } else if filename.ends_with(".yaml") || filename.ends_with(".yml") {
            let value: serde_yaml_ng::Value =
                serde_yaml_ng::from_str(&fs::read_to_string(path)?)
                    .with_context(|| format!("Invalid dataset YAML: {name}"))?;
            if let Some(names) = value.get("names") {
                let mut result = Vec::new();
                if let Some(names) = names.as_sequence() {
                    for name in names {
                        result.push(
                            name.as_str()
                                .context("Class names must be strings")?
                                .to_owned(),
                        );
                    }
                } else if let Some(names) = names.as_mapping() {
                    let mut ordered = BTreeMap::new();
                    for (key, name) in names {
                        let id = key
                            .as_u64()
                            .or_else(|| key.as_str()?.parse().ok())
                            .context("Class IDs must be integers")?;
                        ordered.insert(
                            id,
                            name.as_str()
                                .context("Class names must be strings")?
                                .to_owned(),
                        );
                    }
                    for (index, (id, name)) in ordered.into_iter().enumerate() {
                        ensure!(
                            id == index as u64,
                            "Dataset class IDs must start at zero and be consecutive"
                        );
                        result.push(name);
                    }
                } else {
                    bail!("Dataset YAML names must be a list or class-ID dictionary");
                }
                candidates.push(result);
            }
        }
    }
    let Some(first) = candidates.first() else {
        return Ok(vec![]);
    };
    ensure!(
        candidates.iter().all(|names| names == first),
        "Dataset class files disagree; provide one class list"
    );
    validate_names(first.clone())
}

fn validate_names(names: Vec<String>) -> Result<Vec<String>> {
    ensure!(names.len() <= 10_000, "Too many YOLO classes");
    let mut seen = BTreeSet::new();
    let names: Vec<_> = names
        .into_iter()
        .map(|name| name.trim().to_owned())
        .collect();
    for name in &names {
        ensure!(
            !name.is_empty()
                && name.len() <= 200
                && !name.chars().any(char::is_control)
                && seen.insert(name),
            "Class names must be nonempty, unique, and at most 200 bytes"
        );
    }
    Ok(names)
}

fn boxes(text: &str, name: &str) -> Result<Vec<Value>> {
    let mut result = vec![];
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split_whitespace().collect();
        ensure!(
            parts.len() == 5,
            "{name}, line {}: expected class x_center y_center width height (YOLO detection boxes)",
            line_number + 1
        );
        let class: usize = parts[0]
            .parse()
            .with_context(|| format!("{name}: class ID must be a nonnegative integer"))?;
        ensure!(
            class < 10_000,
            "{name}: class ID exceeds the supported range"
        );
        let values: Vec<f64> = parts[1..]
            .iter()
            .map(|v| v.parse().context("YOLO coordinates must be numbers"))
            .collect::<Result<_>>()?;
        ensure!(
            values
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                && values[2] > 0.0
                && values[3] > 0.0
                && values[0] - values[2] / 2.0 >= -1e-6
                && values[0] + values[2] / 2.0 <= 1.0 + 1e-6
                && values[1] - values[3] / 2.0 >= -1e-6
                && values[1] + values[3] / 2.0 <= 1.0 + 1e-6,
            "{name}, line {}: YOLO boxes must use finite normalized coordinates inside the image",
            line_number + 1
        );
        result.push(json!({"class_id":class,"x_center":values[0],"y_center":values[1],"width":values[2],"height":values[3]}));
        ensure!(result.len() <= 100_000, "Too many boxes in {name}");
    }
    Ok(result)
}

impl Store {
    pub(crate) fn yolo_upload_directory(&self, project: &str) -> Result<tempfile::TempDir> {
        let (folder, manifest) = self.manifest(project)?;
        Self::editable(&manifest)?;
        let parent = contained_path(&folder, "datasets")?;
        fs::create_dir_all(&parent)?;
        Ok(tempfile::Builder::new()
            .prefix(".yolo-upload-")
            .tempdir_in(parent)?)
    }

    pub fn import_yolo(
        &self,
        project: &str,
        name: &str,
        uploaded: &[(String, PathBuf)],
        supplied_names: &[String],
        actor: &crate::accounts::Principal,
    ) -> Result<Value> {
        ensure!(
            !uploaded.is_empty(),
            "Choose YOLO labels and their PNG/JPEG images, or a ZIP dataset"
        );
        let staging = self.yolo_upload_directory(project)?;
        let mut files = Vec::new();
        if uploaded.len() == 1 && uploaded[0].0.to_ascii_lowercase().ends_with(".zip") {
            let mut archive = zip::ZipArchive::new(fs::File::open(&uploaded[0].1)?)?;
            ensure!(archive.len() <= 100_000, "ZIP contains too many files");
            let mut total = 0u64;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i)?;
                ensure!(
                    entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                    "ZIP symlinks are not supported"
                );
                if entry.is_dir() {
                    continue;
                }
                let name = upload_name(entry.name())?;
                total = total
                    .checked_add(entry.size())
                    .context("ZIP size overflow")?;
                ensure!(
                    entry.size() <= 512 * 1024 * 1024 && total <= 2 * 1024 * 1024 * 1024,
                    "Dataset ZIP exceeds the import size limit"
                );
                let path = staging.path().join(format!("source-{i}"));
                let mut output = fs::File::create(&path)?;
                let count =
                    std::io::copy(&mut entry.by_ref().take(512 * 1024 * 1024 + 1), &mut output)?;
                ensure!(
                    count == entry.size(),
                    "ZIP entry size does not match its contents"
                );
                files.push((name, path));
            }
        } else {
            ensure!(
                !uploaded
                    .iter()
                    .any(|(name, _)| name.to_ascii_lowercase().ends_with(".zip")),
                "Import a ZIP by itself, or choose labels and images together"
            );
            for (name, path) in uploaded {
                files.push((upload_name(name)?, path.clone()));
            }
        }
        ensure!(files.len() <= 100_000, "Too many dataset files");
        let mut paths = BTreeSet::new();
        for (name, path) in &files {
            ensure!(paths.insert(name), "Duplicate dataset path: {name}");
            let size = fs::metadata(path)?.len();
            let text = name.ends_with(".txt")
                || name.ends_with(".yaml")
                || name.ends_with(".yml")
                || name.ends_with(".names");
            ensure!(
                !text || size <= 4 * 1024 * 1024,
                "Dataset text file is too large: {name}"
            );
        }
        let mut names = class_names(&files, supplied_names)?;
        let mut labels = BTreeMap::new();
        let mut images = BTreeMap::new();
        for (name, path) in &files {
            if name
                .split('/')
                .any(|part| part == "__MACOSX" || part.starts_with("._"))
            {
                continue;
            }
            let extension = Path::new(name)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if matches!(extension.as_str(), "png" | "jpg" | "jpeg") {
                ensure!(
                    images.insert(pairing_key(name), (name, path)).is_none(),
                    "Images have ambiguous matching names: {name}"
                );
            }
        }
        ensure!(
            !images.is_empty(),
            "No PNG/JPEG frames found. Include the images corresponding to the YOLO labels"
        );
        for (name, path) in &files {
            if name
                .split('/')
                .any(|part| part == "__MACOSX" || part.starts_with("._"))
            {
                continue;
            }
            let basename = Path::new(name)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            if !basename.ends_with(".txt") || basename == "classes.txt" {
                continue;
            }
            let key = pairing_key(name);
            let in_labels = name.split('/').any(|part| part == "labels");
            let text = fs::read_to_string(path)?;
            let first = text
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("");
            let looks_like_label = first.is_empty()
                || first
                    .split_whitespace()
                    .next()
                    .is_some_and(|word| word.parse::<usize>().is_ok());
            if images.contains_key(&key)
                || in_labels
                || (looks_like_label
                    && !matches!(basename.as_str(), "train.txt" | "val.txt" | "test.txt"))
            {
                ensure!(
                    labels.insert(key, (name, path)).is_none(),
                    "Labels have ambiguous matching names: {name}"
                );
            }
        }
        for key in labels.keys() {
            ensure!(
                images.contains_key(key),
                "Label file has no corresponding image: {}",
                labels[key].0
            );
        }
        let dataset_id = Uuid::new_v4().to_string();
        let publish = staging.path().join("dataset");
        fs::create_dir(&publish)?;
        let base = format!("datasets/{dataset_id}");
        let mut samples = BTreeMap::<String, Map<String, Value>>::new();
        let mut splits = BTreeMap::<String, usize>::new();
        let (mut box_count, mut negative_count, mut max_class) = (0, 0, None::<usize>);
        for (index, (key, (name, path))) in images.iter().enumerate() {
            let bytes = fs::read(path)?;
            let reader =
                image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
            ensure!(
                matches!(
                    reader.format(),
                    Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
                ),
                "Frame must be a valid PNG/JPEG: {name}"
            );
            let (width, height) = reader
                .into_dimensions()
                .with_context(|| format!("Invalid image: {name}"))?;
            ensure!(
                width > 0 && height > 0 && (width as u64) * (height as u64) <= 100_000_000,
                "Excessive image dimensions: {name}"
            );
            image::load_from_memory(&bytes).with_context(|| format!("Invalid image: {name}"))?;
            let label = labels.get(key);
            let text = label
                .map(|(_, p)| fs::read_to_string(p))
                .transpose()?
                .unwrap_or_default();
            let annotations = boxes(&text, name)?;
            for b in &annotations {
                max_class = Some(
                    max_class.map_or(b["class_id"].as_u64().unwrap() as usize, |old| {
                        old.max(b["class_id"].as_u64().unwrap() as usize)
                    }),
                );
            }
            box_count += annotations.len();
            if annotations.is_empty() {
                negative_count += 1;
            }
            let split = name
                .split('/')
                .find_map(|part| match part {
                    "train" => Some("train"),
                    "valid" | "val" => Some("val"),
                    "test" => Some("test"),
                    _ => None,
                })
                .unwrap_or("train");
            *splits.entry(split.into()).or_default() += 1;
            let extension = Path::new(name)
                .extension()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            let image_relative = format!("images/{split}/{index:08}.{extension}");
            let label_relative = format!("labels/{split}/{index:08}.txt");
            fs::create_dir_all(publish.join(format!("images/{split}")))?;
            fs::create_dir_all(publish.join(format!("labels/{split}")))?;
            fs::write(publish.join(&image_relative), bytes)?;
            fs::write(publish.join(&label_relative), text)?;
            samples.entry(format!("{:012}",index as u64 / crate::storage::PAGE_SIZE)).or_default()
                .insert(index.to_string(),json!({"index":index,"source_file":name,"split":split,"image":format!("{base}/{image_relative}"),"label":format!("{base}/{label_relative}"),"width":width,"height":height,"boxes":annotations}));
        }
        if names.is_empty() {
            names = (0..max_class.map_or(0, |max| max + 1))
                .map(|i| format!("Class {i}"))
                .collect();
        }
        ensure!(
            max_class.is_none_or(|class| class < names.len()),
            "A label references a class not defined in the class list"
        );
        fs::write(publish.join("classes.txt"), names.join("\n") + "\n")?;
        let mut config = json!({"path":".","names":names});
        for split in splits.keys() {
            config[split] = json!(format!("images/{split}"));
        }
        fs::write(
            publish.join("data.yaml"),
            serde_yaml_ng::to_string(&config)?,
        )?;
        let pages = crate::storage::write_pages(staging.path(), "dataset/samples", samples)?;
        let mut pages = pages;
        for page in pages.as_object_mut().unwrap().values_mut() {
            page["path"] = json!(field(page, "path")?.replacen("dataset/", &format!("{base}/"), 1));
        }
        let dataset = json!({"uuid":dataset_id,"kind":"imported-yolo","name":valid_name(&json!({"name":name}))?,"format":"yolo","entry_count":images.len(),"class_names":names,"split_counts":splits,"box_count":box_count,"negative_count":negative_count,"created_at":now(),"created_by_name":actor.name,"sample_pages":pages});
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(project)?;
        let _lock = Self::lock_project(&folder)?;
        self.accounts
            .require_project(actor, &folder, "datasets.generate")?;
        let mut manifest = self.cached_manifest(&folder)?;
        Self::editable(&manifest)?;
        let previous = manifest.clone();
        ensure!(
            !manifest["datasets"]
                .as_object()
                .unwrap()
                .values()
                .any(|d| d["name"] == dataset["name"]),
            "A dataset with this name already exists"
        );
        let destination = contained_path(&folder, &base)?;
        fs::rename(publish, &destination)?;
        manifest["datasets"][&dataset_id] = dataset.clone();
        if let Err(error) = crate::storage::save_manifest_delta(&folder, &manifest, &previous) {
            let _ = fs::remove_dir_all(destination);
            return Err(error);
        }
        Ok(json!({"dataset":dataset,"project":self.detail(&folder,&manifest)}))
    }

    pub fn yolo_samples(
        &self,
        project: &str,
        dataset_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        canonical_uuid(dataset_id)?;
        let (folder, manifest) = self.manifest(project)?;
        let dataset = &manifest["datasets"][dataset_id];
        ensure!(
            dataset["kind"] == "imported-yolo",
            "Dataset is not an imported YOLO dataset"
        );
        let limit = limit.clamp(1, 100);
        let end = offset
            .saturating_add(limit)
            .min(dataset["entry_count"].as_u64().unwrap_or(0) as usize);
        let pages: Map<_, _> = dataset["sample_pages"]
            .as_object()
            .context("Missing imported samples")?
            .iter()
            .filter(|(group, _)| {
                group.parse::<usize>().is_ok_and(|n| {
                    n >= offset / crate::storage::PAGE_SIZE as usize
                        && n <= end / crate::storage::PAGE_SIZE as usize
                })
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let rows = crate::storage::read_pages(
            &folder,
            &format!("datasets/{dataset_id}/samples"),
            &json!(pages),
        )?;
        let values: Vec<_> = (offset..end)
            .filter_map(|i| rows.get(&i.to_string()).cloned())
            .collect();
        Ok(
            json!({"samples":values,"total":dataset["entry_count"],"class_names":dataset["class_names"]}),
        )
    }

    pub fn yolo_image(
        &self,
        project: &str,
        dataset: &str,
        index: usize,
    ) -> Result<(Vec<u8>, String)> {
        let samples = self.yolo_samples(project, dataset, index, 1)?;
        let row = samples["samples"]
            .get(0)
            .context("Imported frame not found")?;
        let folder = self.folder(project)?;
        let relative = field(row, "image")?;
        ensure!(
            relative.starts_with(&format!("datasets/{dataset}/images/")),
            "Imported image path does not match dataset"
        );
        let path = contained_path(&folder, relative)?;
        let mime = if path.extension().is_some_and(|ext| ext == "png") {
            "image/png"
        } else {
            "image/jpeg"
        };
        Ok((fs::read(path)?, mime.into()))
    }

    pub fn yolo_archive(&self, project: &str, dataset: &str) -> Result<Vec<u8>> {
        canonical_uuid(dataset)?;
        let (folder, manifest) = self.manifest(project)?;
        ensure!(
            manifest["datasets"][dataset]["kind"] == "imported-yolo",
            "Dataset is not imported YOLO"
        );
        let root = contained_path(&folder, &format!("datasets/{dataset}"))?;
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for relative in super::list_files(&root)? {
            let name = relative;
            if !(name.starts_with("images/")
                || name.starts_with("labels/")
                || matches!(name.as_ref(), "classes.txt" | "data.yaml"))
            {
                continue;
            }
            let path = contained_path(&root, &name)?;
            archive.start_file(
                &name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            archive.write_all(&fs::read(path)?)?;
        }
        Ok(archive.finish()?.into_inner())
    }
}
