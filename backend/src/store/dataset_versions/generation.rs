//! Materialize fixed version membership. Live annotations and label definitions
//! never participate in a production build.
use super::*;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
};

struct SourceCache {
    index: std::sync::Arc<Value>,
    cache: std::path::PathBuf,
    source_sha: Value,
}
fn text_changed(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.is_file() && fs::read(path)? == bytes {
        return Ok(());
    }
    fs::create_dir_all(path.parent().context("Missing output directory")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn translated_record(
    readers: &mut HashMap<String, BufReader<File>>,
    context: &SourceCache,
    input: &Value,
    number: usize,
) -> Result<Value> {
    let channel = field(input, "channel_id")?;
    let frame = &context.index["topics"][channel]["frames"][number];
    let offset = frame["offset"]
        .as_u64()
        .context("Sensor frame has no record offset")?;
    let path = contained_path(
        &context.cache,
        &format!("frames/channel_{channel}/records.jsonl"),
    )?;
    let key = path.to_string_lossy().into_owned();
    if !readers.contains_key(&key) {
        if readers.len() >= 32 {
            readers.clear();
        }
        readers.insert(key.clone(), BufReader::new(File::open(path)?));
    }
    let reader = readers.get_mut(&key).unwrap();
    reader.seek(SeekFrom::Start(offset))?;
    let mut line = String::new();
    reader.take(16 * 1024 * 1024).read_line(&mut line)?;
    let value: Value = serde_json::from_str(&line)?;
    let rule = input["translation"].as_str().unwrap_or("");
    if rule.trim().is_empty() {
        Ok(value)
    } else {
        crate::engine::translate_record(&value, rule)
    }
}

impl Store {
    pub fn build_dataset_version(
        &self,
        project: &str,
        dataset: &str,
        number: u64,
        request: &Value,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(project)?;
        let _lock = Self::lock_project(&folder)?;
        let version = self.dataset_version(project, dataset, number)?;
        let format = request["format"]
            .as_str()
            .or(version["default_format"].as_str())
            .unwrap_or("csv");
        ensure!(
            ["csv", "yolo"].contains(&format),
            "Choose CSV or YOLO output"
        );
        let base = format!("datasets/{dataset}/versions/{number:012}/generated/{format}");
        let output = contained_path(&folder, &base)?;
        fs::create_dir_all(&output)?;
        let manifest_path = contained_path(&folder, &format!("{base}/manifest.json"))?;
        let version_hash = digest(&version)?;
        let existing = read_json(&manifest_path)
            .ok()
            .filter(|m| m["source_version_sha256"] == version_hash && m["builder_version"] == 2);
        if request["repair"] != true {
            if let Some(previous) = &existing {
                let pages_present = previous["sample_pages"].as_object().is_some_and(|pages| {
                    pages.values().all(|p| {
                        p["path"]
                            .as_str()
                            .is_some_and(|p| contained_path(&folder, p).is_ok_and(|p| p.is_file()))
                    })
                });
                let artifact = if format == "csv" {
                    "samples.csv"
                } else {
                    "data.yaml"
                };
                if pages_present && output.join(artifact).is_file() {
                    return Ok(
                        json!({"path":base,"dataset_version":number,"sample_count":version["sample_count"],"updated_samples":0,"copied_images":0,"format":format,"provenance":version["source_revisions"]}),
                    );
                }
            }
        }
        let mut contexts: HashMap<String, SourceCache> = HashMap::new();
        let mut readers = HashMap::new();
        let mut generated_pages = serde_json::Map::new();
        let mut csv_file = tempfile::NamedTempFile::new_in(&output)?;
        let mut csv = BufWriter::new(csv_file.as_file_mut());
        let mut schemas: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut updated = 0u64;
        let mut copied = 0u64;
        let value_columns: Vec<String> = serde_json::from_value(version["columns"].clone())?;
        let mut columns = vec![
            "mcap_uuid".to_owned(),
            "approved_revision".into(),
            "channel_id".into(),
            "frame".into(),
            "time_ns".into(),
            "inputs".into(),
            "annotation".into(),
        ];
        columns.extend(value_columns.iter().cloned());
        writeln!(
            csv,
            "{}\r",
            columns
                .iter()
                .map(|c| super::super::csv_cell(&json!(c)))
                .collect::<Vec<_>>()
                .join(",")
        )?;
        for (bucket, page) in version["sample_pages"]
            .as_object()
            .context("Invalid immutable membership")?
        {
            let source_rows = read_pages(
                &folder,
                &format!("datasets/{dataset}/versions/{number:012}/samples"),
                &json!({bucket:page}),
            )?;
            let mut rows = serde_json::Map::new();
            let mut sensor_bytes = Vec::new();
            let sensor_path = format!("{base}/sensors/{bucket}.jsonl");
            for (key, mut sample) in source_rows {
                let mut generated_inputs = serde_json::Map::new();
                for (input_name, input) in sample["inputs"]
                    .as_object()
                    .context("Sample requires inputs")?
                {
                    let source_project = field(input, "source_project_uuid")?;
                    let recording = field(input, "recording_id")?;
                    let context_key = format!("{source_project}/{recording}");
                    if !contexts.contains_key(&context_key) {
                        let source =
                            self.recording_source(source_project, recording, false, false)?;
                        ensure!(
                            source.item["sha256"] == input["source_sha256"],
                            "Dataset source SHA-256 does not match approved revision"
                        );
                        let wanted: Vec<_> = source.index["topics"]
                            .as_object()
                            .into_iter()
                            .flatten()
                            .filter(|(_, topic)| modality(topic).0 == "depth")
                            .map(|(id, _)| id.clone())
                            .collect();
                        if !wanted.is_empty() {
                            let raw = contained_path(&source.folder, field(&source.item, "path")?)?;
                            crate::engine::ensure_native_training_images(
                                &raw,
                                &source.cache,
                                &source.index,
                                &wanted,
                            )?;
                        }
                        contexts.insert(
                            context_key.clone(),
                            SourceCache {
                                index: source.index,
                                cache: source.cache,
                                source_sha: source.item["sha256"].clone(),
                            },
                        );
                    }
                    let context = &contexts[&context_key];
                    ensure!(
                        context.source_sha == input["source_sha256"],
                        "Dataset source identity does not match"
                    );
                    let mut generated = input.clone();
                    if input["kind"] == "image" {
                        let original = contained_path(
                            &context.cache,
                            &format!("frames/{}", field(input, "image")?),
                        )?;
                        let image_path = format!("{base}/images/{key}_{input_name}.png");
                        let destination = contained_path(&folder, &image_path)?;
                        if !destination.is_file() {
                            fs::create_dir_all(destination.parent().unwrap())?;
                            fs::copy(&original, &destination)?;
                            copied += 1;
                        }
                        generated["image"] = json!(image_path);
                        let tensor_path = format!("{base}/tensors/{key}_{input_name}.npy");
                        if let Some(mut tensor) = crate::engine::write_training_tensor(
                            &original,
                            &contained_path(&folder, &tensor_path)?,
                            input["modality"] == "depth",
                        )? {
                            tensor["path"] = json!(tensor_path);
                            generated["tensor"] = tensor;
                        }
                    } else {
                        let number =
                            input["frame"].as_u64().context("Input requires a frame")? as usize;
                        let mut values = vec![];
                        if input["window"].is_object() {
                            let start = input["window"]["start_frame"]
                                .as_u64()
                                .context("Invalid sensor window")?
                                as usize;
                            let end = input["window"]["end_frame"]
                                .as_u64()
                                .context("Invalid sensor window")?
                                as usize;
                            for n in start..=end {
                                values.push(translated_record(&mut readers, context, input, n)?);
                            }
                        } else if input["interpolation"].is_object() {
                            let r = &input["interpolation"];
                            let left = translated_record(
                                &mut readers,
                                context,
                                input,
                                r["left_frame"]
                                    .as_u64()
                                    .context("Invalid interpolation frame")?
                                    as usize,
                            )?;
                            let right = translated_record(
                                &mut readers,
                                context,
                                input,
                                r["right_frame"]
                                    .as_u64()
                                    .context("Invalid interpolation frame")?
                                    as usize,
                            )?;
                            let fallback = translated_record(&mut readers, context, input, number)?;
                            let numerator = field(r, "numerator")?.parse::<i128>()?;
                            let denominator = field(r, "denominator")?.parse::<i128>()?;
                            values.push(crate::engine::interpolate_records(
                                &left,
                                &right,
                                &fallback,
                                numerator,
                                denominator,
                            )?);
                        } else {
                            values.push(translated_record(&mut readers, context, input, number)?);
                        }
                        for value in &values {
                            collect_numeric_fields(
                                value,
                                "",
                                schemas.entry(input_name.clone()).or_default(),
                            );
                        }
                        let offset = sensor_bytes.len();
                        let payload = json!({"values":values,"time_ns":input["synchronization"]["target_time_ns"],"modality":input["modality"],"window":input["window"]});
                        serde_json::to_writer(&mut sensor_bytes, &payload)?;
                        sensor_bytes.push(b'\n');
                        generated["records"] = json!(sensor_path);
                        generated["offset"] = json!(offset);
                    }
                    generated_inputs.insert(input_name.clone(), generated);
                }
                sample["inputs"] = json!(generated_inputs);
                if let Some(primary) = sample["primary_input"].as_str() {
                    if let Some(image) = sample["inputs"][primary]["image"].as_str() {
                        sample["image"] = json!(image);
                    }
                }
                if format == "yolo" {
                    ensure!(
                        sample["image"].is_string(),
                        "YOLO requires a selected primary image modality"
                    );
                    ensure!(sample["annotation"]["labels"].as_array().is_none_or(Vec::is_empty),"Classification revisions use CSV or PyTorch; YOLO output supports bounding boxes");
                    let width = sample["width"].as_f64().context("Missing image width")?;
                    let height = sample["height"].as_f64().context("Missing image height")?;
                    let primary_image = field(&sample, "image")?;
                    let file = Path::new(primary_image)
                        .file_stem()
                        .context("Invalid image name")?
                        .to_string_lossy();
                    let mut yolo = String::new();
                    for bbox in sample["annotation"]["boxes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        let x = bbox["x"].as_f64().context("Invalid box")?;
                        let y = bbox["y"].as_f64().context("Invalid box")?;
                        let w = bbox["width"].as_f64().context("Invalid box")?;
                        let h = bbox["height"].as_f64().context("Invalid box")?;
                        yolo.push_str(&format!(
                            "{} {:.10} {:.10} {:.10} {:.10}\n",
                            bbox["class_id"].as_u64().context("Invalid class")?,
                            (x + w / 2.0) / width,
                            (y + h / 2.0) / height,
                            w / width,
                            h / height
                        ));
                    }
                    text_changed(
                        &contained_path(&folder, &format!("{base}/labels/{file}.txt"))?,
                        yolo.as_bytes(),
                    )?;
                }
                let mut cells = vec![
                    sample["mcap_uuid"].clone(),
                    sample["approved_revision"]["revision"].clone(),
                    sample["channel_id"].clone(),
                    sample["frame"].clone(),
                    sample["time_ns"].clone(),
                    sample["inputs"].clone(),
                    sample["annotation"].clone(),
                ];
                cells.extend(value_columns.iter().map(|c| sample["values"][c].clone()));
                writeln!(
                    csv,
                    "{}\r",
                    cells
                        .iter()
                        .map(super::super::csv_cell)
                        .collect::<Vec<_>>()
                        .join(",")
                )?;
                rows.insert(key, sample);
            }
            if !sensor_bytes.is_empty() {
                text_changed(&contained_path(&folder, &sensor_path)?, &sensor_bytes)?;
            }
            let materialized = write_pages(
                &folder,
                &format!("{base}/samples"),
                BTreeMap::from([(bucket.clone(), rows)]),
            )?;
            if existing
                .as_ref()
                .is_none_or(|old| old["sample_pages"][bucket] != materialized[bucket])
            {
                updated += page["count"].as_u64().unwrap_or(0);
            }
            generated_pages.extend(materialized.as_object().unwrap().clone());
        }
        csv.flush()?;
        drop(csv);
        if format == "csv" {
            csv_file.as_file().sync_all()?;
            csv_file
                .persist(contained_path(&folder, &format!("{base}/samples.csv"))?)
                .map_err(|e| e.error)?;
        }
        if format == "yolo" {
            let names: Vec<_> = version["labels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["name"].clone())
                .collect();
            text_changed(
                &contained_path(&folder, &format!("{base}/data.yaml"))?,
                format!(
                    "path: .\ntrain: images\nnames: {}\n",
                    serde_json::to_string(&names)?
                )
                .as_bytes(),
            )?;
        }
        let generated = json!({"format_version":1,"builder_version":2,"dataset_uuid":dataset,"dataset_version":number,"project_uuid":project,"format":format,"sample_count":version["sample_count"],"sample_pages":generated_pages,"classes":version["labels"].as_array().unwrap().iter().map(|l|l["name"].clone()).collect::<Vec<_>>(),"columns":value_columns,"feature_schemas":schemas,"modalities":version["selection"]["modalities"],"source_revisions":version["source_revisions"],"source_version_sha256":version_hash});
        write_changed(&manifest_path, &generated)?;
        Ok(
            json!({"path":base,"dataset_version":number,"sample_count":version["sample_count"],"updated_samples":updated,"copied_images":copied,"format":format,"provenance":version["source_revisions"]}),
        )
    }
    pub fn dataset_version_csv(
        &self,
        project: &str,
        dataset: &str,
        number: u64,
    ) -> Result<Vec<u8>> {
        let folder = self.folder(project)?;
        let version = self.dataset_version(project, dataset, number)?;
        let path = contained_path(
            &folder,
            &format!("datasets/{dataset}/versions/{number:012}/generated/csv/samples.csv"),
        )?;
        ensure!(
            path.is_file(),
            "Build CSV for this dataset version before downloading"
        );
        let manifest = read_json(&contained_path(
            &folder,
            &format!("datasets/{dataset}/versions/{number:012}/generated/csv/manifest.json"),
        )?)?;
        ensure!(
            manifest["source_version_sha256"] == digest(&version)?,
            "Generated CSV does not match immutable dataset version"
        );
        Ok(fs::read(path)?)
    }
}

fn collect_numeric_fields(value: &Value, prefix: &str, names: &mut BTreeSet<String>) {
    match value {
        Value::Object(values) => {
            for (k, v) in values {
                collect_numeric_fields(v, &format!("{prefix}/{k}"), names)
            }
        }
        Value::Array(values) => {
            for (i, v) in values.iter().enumerate() {
                collect_numeric_fields(v, &format!("{prefix}/{i}"), names)
            }
        }
        Value::Number(_) | Value::Bool(_) => {
            names.insert(prefix.into());
        }
        _ => {}
    }
}
