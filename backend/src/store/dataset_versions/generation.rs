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
#[derive(Default)]
struct SensorRecords {
    readers: HashMap<(std::path::PathBuf, String), SensorReader>,
    translations: HashMap<String, crate::engine::CompiledTranslation>,
}
struct SensorReader {
    file: BufReader<File>,
    position: u64,
}
impl SensorRecords {
    fn read(&mut self, context: &SourceCache, input: &Value, number: usize) -> Result<Value> {
        let channel = field(input, "channel_id")?;
        let frame = &context.index["topics"][channel]["frames"][number];
        let offset = frame["offset"]
            .as_u64()
            .context("Sensor frame has no record offset")?;
        let key = (context.cache.clone(), channel.to_owned());
        if !self.readers.contains_key(&key) {
            // Validate paths when opening a source, then reuse that file handle
            // instead of repeating filesystem traversal for every sensor row.
            let path = contained_path(
                &context.cache,
                &format!("frames/channel_{channel}/records.jsonl"),
            )?;
            if self.readers.len() >= 32 {
                self.readers.clear();
            }
            self.readers.insert(
                key.clone(),
                SensorReader {
                    file: BufReader::with_capacity(256 * 1024, File::open(&path)?),
                    position: 0,
                },
            );
        }
        let reader = self.readers.get_mut(&key).unwrap();
        if offset != reader.position {
            let delta = offset as i128 - reader.position as i128;
            if let Ok(delta) = i64::try_from(delta) {
                // BufReader can satisfy nearby seeks inside its existing buffer.
                reader.file.seek_relative(delta)?;
            } else {
                reader.file.seek(SeekFrom::Start(offset))?;
            }
        }
        let mut line = String::new();
        (&mut reader.file)
            .take(16 * 1024 * 1024)
            .read_line(&mut line)?;
        reader.position = offset + line.len() as u64;
        let value: Value = serde_json::from_str(&line)?;
        let rule = input["translation"].as_str().unwrap_or("");
        if rule.trim().is_empty() {
            return Ok(value);
        }
        if !self.translations.contains_key(rule) {
            if self.translations.len() >= 128 {
                self.translations.clear();
            }
            self.translations.insert(
                rule.to_owned(),
                crate::engine::CompiledTranslation::parse(rule)?,
            );
        }
        self.translations[rule].apply_owned(value)
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
        let mut readers = SensorRecords::default();
        let mut generated_pages = serde_json::Map::new();
        let mut csv_file = if format == "csv" {
            Some(tempfile::NamedTempFile::new_in(&output)?)
        } else {
            None
        };
        let mut csv = csv_file
            .as_mut()
            .map(|file| BufWriter::new(file.as_file_mut()));
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
            "split".into(),
        ];
        columns.extend(value_columns.iter().cloned());
        if let Some(csv) = &mut csv {
            writeln!(
                csv,
                "{}\r",
                columns
                    .iter()
                    .map(|c| super::super::csv_cell(&json!(c)))
                    .collect::<Vec<_>>()
                    .join(",")
            )?;
        }
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
                        let split = sample["split"].as_str().unwrap_or("train");
                        ensure!(
                            ["train", "val", "test"].contains(&split),
                            "Invalid sample split"
                        );
                        let image_path = format!("{base}/images/{split}/{key}_{input_name}.png");
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
                                values.push(readers.read(context, input, n)?);
                            }
                        } else if input["interpolation"].is_object() {
                            let r = &input["interpolation"];
                            let left = readers.read(
                                context,
                                input,
                                r["left_frame"]
                                    .as_u64()
                                    .context("Invalid interpolation frame")?
                                    as usize,
                            )?;
                            let right = readers.read(
                                context,
                                input,
                                r["right_frame"]
                                    .as_u64()
                                    .context("Invalid interpolation frame")?
                                    as usize,
                            )?;
                            let fallback = readers.read(context, input, number)?;
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
                            values.push(readers.read(context, input, number)?);
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
                        &contained_path(
                            &folder,
                            &format!(
                                "{base}/labels/{}/{file}.txt",
                                sample["split"].as_str().unwrap_or("train")
                            ),
                        )?,
                        yolo.as_bytes(),
                    )?;
                }
                if let Some(csv) = &mut csv {
                    let mut cells = vec![
                        sample["mcap_uuid"].clone(),
                        sample["approved_revision"]["revision"].clone(),
                        sample["channel_id"].clone(),
                        sample["frame"].clone(),
                        sample["time_ns"].clone(),
                        sample["inputs"].clone(),
                        sample["annotation"].clone(),
                        sample["split"].clone(),
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
                }
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
        if let Some(csv) = &mut csv {
            csv.flush()?;
        }
        drop(csv);
        if let Some(csv_file) = csv_file {
            csv_file.as_file().sync_all()?;
            csv_file
                .persist(contained_path(&folder, &format!("{base}/samples.csv"))?)
                .map_err(|e| e.error)?;
        }
        if format == "yolo" {
            for split in ["train", "val", "test"] {
                fs::create_dir_all(contained_path(&folder, &format!("{base}/images/{split}"))?)?;
                fs::create_dir_all(contained_path(&folder, &format!("{base}/labels/{split}"))?)?;
            }
            let names: Vec<_> = version["labels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["name"].clone())
                .collect();
            text_changed(
                &contained_path(&folder, &format!("{base}/data.yaml"))?,
                format!(
                    "path: .\ntrain: images/train\nval: images/val\ntest: images/test\nnames: {}\n",
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sensor_reads_preserve_offsets_across_windows_and_compile_each_rule_once() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("frames/channel_1")).unwrap();
        let mut bytes = Vec::new();
        let mut frames = Vec::new();
        for value in 0..5 {
            frames.push(json!({"offset":bytes.len()}));
            // Cross the reader's buffer boundary as well as testing nearby seeks.
            serde_json::to_writer(
                &mut bytes,
                &json!({"value":value,"padding":"x".repeat(100_000)}),
            )
            .unwrap();
            bytes.push(b'\n');
        }
        fs::write(dir.path().join("frames/channel_1/records.jsonl"), bytes).unwrap();
        let source = SourceCache {
            index: std::sync::Arc::new(json!({"topics":{"1":{"frames":frames}}})),
            cache: dir.path().to_owned(),
            source_sha: Value::Null,
        };
        let input = json!({"channel_id":"1","translation":"scaled <- source.value * 2"});
        let mut records = SensorRecords::default();
        for number in [0, 1, 2, 1, 2, 3, 4, 0, 4] {
            assert_eq!(
                records.read(&source, &input, number).unwrap(),
                json!({"scaled":number*2})
            );
        }
        assert_eq!(records.translations.len(), 1);
        assert_eq!(records.readers.len(), 1);
        assert_eq!(
            records
                .read(
                    &source,
                    &json!({"channel_id":"1","translation":"scaled <- source.value * 3"}),
                    4
                )
                .unwrap(),
            json!({"scaled":12})
        );
        assert_eq!(records.translations.len(), 2);
    }

    #[test]
    #[ignore = "sensor-window throughput benchmark; use --release --ignored --nocapture"]
    fn sensor_window_throughput() {
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("frames/channel_1")).unwrap();
        let mut bytes = Vec::new();
        let mut frames = Vec::new();
        for value in 0..10_000 {
            frames.push(json!({"offset":bytes.len()}));
            serde_json::to_writer(&mut bytes, &json!({"value":value,"vector":[1,2,3]})).unwrap();
            bytes.push(b'\n');
        }
        let path = dir.path().join("frames/channel_1/records.jsonl");
        fs::write(&path, bytes).unwrap();
        let source = SourceCache {
            index: std::sync::Arc::new(json!({"topics":{"1":{"frames":frames}}})),
            cache: dir.path().to_owned(),
            source_sha: Value::Null,
        };
        let script = "scaled <- source.value * 2\ncomponent <- source.vector[1] + 3";
        let input = json!({"channel_id":"1","translation":script});
        let mut reference = BufReader::new(File::open(path).unwrap());
        let mut expected_sum = 0;
        let started = Instant::now();
        for anchor in 0..5_000 {
            for number in anchor..anchor + 8 {
                let _ = contained_path(&source.cache, "frames/channel_1/records.jsonl").unwrap();
                reference
                    .seek(SeekFrom::Start(
                        source.index["topics"]["1"]["frames"][number]["offset"]
                            .as_u64()
                            .unwrap(),
                    ))
                    .unwrap();
                let mut line = String::new();
                reference.read_line(&mut line).unwrap();
                let value =
                    crate::engine::translate_record(&serde_json::from_str(&line).unwrap(), script)
                        .unwrap();
                expected_sum += value["scaled"].as_u64().unwrap();
            }
        }
        let before = started.elapsed();
        let mut records = SensorRecords::default();
        let mut actual_sum = 0;
        let started = Instant::now();
        for anchor in 0..5_000 {
            for number in anchor..anchor + 8 {
                actual_sum += records.read(&source, &input, number).unwrap()["scaled"]
                    .as_u64()
                    .unwrap();
            }
        }
        let after = started.elapsed();
        assert_eq!(actual_sum, expected_sum);
        eprintln!(
            "40,000 overlapping sensor reads: previous {:.3}s, buffered/compiled {:.3}s ({:.2}x)",
            before.as_secs_f64(),
            after.as_secs_f64(),
            before.as_secs_f64() / after.as_secs_f64()
        );
    }
}
