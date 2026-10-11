//! Structured message flattening, column transformations and CSV creation.
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
};
use uuid::Uuid;

use super::{
    canonical_uuid, contained_path, field, load_manifest, now, registry, valid_name, Recording,
    Store,
};

impl Store {
    pub fn structured_preview(
        &self,
        id: &str,
        recording_id: &str,
        channel: &str,
        request: &Value,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let recording = self.recording_inner(id, recording_id)?;
        let rows = structured_rows(&recording, channel)?;
        let fields: std::collections::BTreeSet<_> = rows
            .iter()
            .flat_map(|row| row.as_object().unwrap().keys().cloned())
            .collect();
        let offset = request["offset"].as_u64().unwrap_or(0) as usize;
        let limit = request["limit"].as_u64().unwrap_or(50).clamp(1, 200) as usize;
        let frames = recording.index["topics"][channel]["frames"]
            .as_array()
            .unwrap();
        let preview: Vec<_> = rows.iter().enumerate().skip(offset).take(limit).map(|(number, values)| json!({"index":number,"time_ns":frames[number]["time_ns"].as_u64().map(|value| value.to_string()),"values":values})).collect();
        Ok(json!({"fields":fields,"rows":preview,"total":rows.len(),"offset":offset}))
    }

    pub fn save_structured_dataset(
        &self,
        id: &str,
        recording_id: &str,
        channel: &str,
        request: &Value,
    ) -> Result<Value> {
        self.save_structured_dataset_inner(id, recording_id, channel, request, None)
    }
    pub fn save_structured_dataset_as(
        &self,
        id: &str,
        recording_id: &str,
        channel: &str,
        request: &Value,
        actor: &crate::accounts::Principal,
    ) -> Result<Value> {
        self.save_structured_dataset_inner(id, recording_id, channel, request, Some(actor))
    }
    fn save_structured_dataset_inner(
        &self,
        id: &str,
        recording_id: &str,
        channel: &str,
        request: &Value,
        actor: Option<&crate::accounts::Principal>,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        if let Some(actor) = actor {
            self.accounts
                .require_project(actor, &folder, "datasets.generate")?;
            if let Some(dataset_id) = request["dataset_uuid"].as_str() {
                self.check_dataset_task_edit(&folder, dataset_id, actor)?;
            }
        }
        let recording = self.recording_for_edit(id, recording_id)?;
        Self::editable(&recording.project)?;
        let name = valid_name(request)?;
        let rows = structured_rows(&recording, channel)?;
        ensure!(!rows.is_empty(), "Topic has no message rows");
        let columns = request["columns"].as_array().context("Provide columns")?;
        ensure!(
            !columns.is_empty() && columns.len() <= 256,
            "Select between 1 and 256 columns"
        );
        let fields: HashSet<_> = rows
            .iter()
            .flat_map(|row| row.as_object().unwrap().keys())
            .collect();
        let mut names = HashSet::new();
        let mut ids = HashSet::new();
        for column in columns {
            let name = valid_name(column)?;
            ensure!(names.insert(name), "Column names must be unique");
            ensure!(
                ids.insert(field(column, "id")?),
                "Column IDs must be unique"
            );
            ensure!(
                column["source"].is_null() || column["source"].is_string(),
                "Column source must be a field path or null"
            );
            if let Some(list_id) = column["label_list"].as_str().filter(|id| !id.is_empty()) {
                ensure!(
                    recording.project["label_lists"].get(list_id).is_some(),
                    "Label list not found"
                );
            }
            if let Some(source) = column["source"].as_str() {
                ensure!(
                    fields.contains(&source.to_owned()),
                    "Unknown source field: {source}"
                );
            }
            ensure!(
                ["identity", "text", "number", "boolean", "scale"]
                    .contains(&column["transform"].as_str().unwrap_or("identity")),
                "Unknown column transformation"
            );
        }
        ensure!(
            request.get("overrides").is_none_or(Value::is_object),
            "Custom row values must be an object"
        );
        if let Some(overrides) = request["overrides"].as_object() {
            for (number, values) in overrides {
                ensure!(
                    number
                        .parse::<usize>()
                        .is_ok_and(|number| number < rows.len()),
                    "Custom row is outside the topic"
                );
                for (column_id, value) in values
                    .as_object()
                    .context("Custom values must be an object")?
                {
                    ensure!(value.is_string(), "Custom cell values must be strings");
                    ensure!(
                        columns
                            .iter()
                            .any(|column| column["id"] == column_id.as_str()
                                && column["source"].is_null()),
                        "Only custom columns can be edited per row"
                    );
                }
            }
        }
        let mut manifest = load_manifest(&folder)?;
        let uuid = if let Some(dataset_id) = request["dataset_uuid"].as_str() {
            canonical_uuid(dataset_id)?;
            let existing = manifest["datasets"]
                .get(dataset_id)
                .context("Dataset not found")?;
            ensure!(
                existing["format"] == "csv"
                    && existing["source_recording_uuid"] == recording_id
                    && existing["source_channel"] == channel
                    && existing["source_sha256"] == recording.item["sha256"],
                "Dataset source does not match this topic"
            );
            dataset_id.to_owned()
        } else {
            Uuid::new_v4().to_string()
        };
        ensure!(
            !registry(&manifest, "datasets")?
                .iter()
                .any(|(id, item)| id != &uuid && item["name"] == name),
            "A dataset with this name already exists"
        );
        let mut entries = Map::new();
        let mut csv = String::new();
        csv.push_str(
            &columns
                .iter()
                .map(|column| csv_cell(&column["name"]))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push_str("\r\n");
        let frames = recording.index["topics"][channel]["frames"]
            .as_array()
            .unwrap();
        for (number, row) in rows.iter().enumerate() {
            let mut values = Map::new();
            let mut cells = Vec::new();
            for column in columns {
                let value = if let Some(source) = column["source"].as_str() {
                    row.get(source).cloned().unwrap_or(Value::Null)
                } else {
                    request["overrides"][number.to_string()][field(column, "id")?]
                        .as_str()
                        .map(|value| json!(value))
                        .unwrap_or_else(|| column.get("default").cloned().unwrap_or(Value::Null))
                };
                let value = transform_cell(value, column).with_context(|| {
                    format!("Message {}, column {}", number + 1, column["name"])
                })?;
                cells.push(csv_cell(&value));
                values.insert(field(column, "name")?.trim().to_owned(), value);
            }
            csv.push_str(&cells.join(","));
            csv.push_str("\r\n");
            entries.insert(format!("{recording_id}:{channel}:{number}"), json!({"raw_data":{"mcap":recording.item["path"],"sha256":recording.item["sha256"]},"mcap_uuid":recording_id,"channel_id":channel,"topic":recording.index["topics"][channel]["topic"],"frame":number,"time_ns":frames[number]["time_ns"],"values":values}));
        }
        let relative = format!("datasets/{uuid}/data.csv");
        let csv_path = contained_path(&folder, &relative)?;
        fs::create_dir_all(csv_path.parent().unwrap())?;
        let previous_csv = if csv_path.is_file() {
            Some(fs::read(&csv_path)?)
        } else {
            None
        };
        let mut temporary = tempfile::NamedTempFile::new_in(csv_path.parent().unwrap())?;
        temporary.write_all(csv.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(&csv_path).map_err(|error| error.error)?;
        let created_at = manifest["datasets"][&uuid]
            .get("created_at")
            .cloned()
            .unwrap_or_else(|| json!(now()));
        manifest["datasets"][&uuid] = json!({"uuid":uuid,"name":name,"format":"csv","columns":columns,"overrides":request.get("overrides").cloned().unwrap_or(json!({})),"csv_path":relative,"entries":entries,"created_at":created_at,"source_recording_uuid":recording_id,"source_channel":channel,"source_sha256":recording.item["sha256"]});
        if let Err(error) = crate::storage::save_manifest(&folder, &manifest) {
            if let Some(previous) = previous_csv {
                let _ = fs::write(&csv_path, previous);
            } else {
                let _ = fs::remove_file(&csv_path);
            }
            return Err(error);
        }
        Ok(self.detail(&folder, &manifest))
    }
}

pub(super) fn flatten_message(value: &Value, path: &str, fields: &mut Map<String, Value>) {
    match value {
        Value::Object(items) if !items.is_empty() => {
            for (key, value) in items {
                flatten_message(
                    value,
                    &format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                    fields,
                );
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, value) in items.iter().enumerate() {
                flatten_message(value, &format!("{path}/{index}"), fields);
            }
        }
        _ => {
            // Keep 64-bit integers exact in browser previews and CSV exports.
            let cell = if value
                .as_u64()
                .is_some_and(|value| value > 9_007_199_254_740_991)
                || value
                    .as_i64()
                    .is_some_and(|value| value < -9_007_199_254_740_991)
            {
                json!(value.to_string())
            } else {
                value.clone()
            };
            fields.insert(
                if path.is_empty() {
                    "/value".to_owned()
                } else {
                    path.to_owned()
                },
                cell,
            );
        }
    }
}

pub(super) fn structured_rows(recording: &Recording, channel: &str) -> Result<Vec<Value>> {
    let topic = recording.index["topics"]
        .get(channel)
        .context("Topic not found")?;
    ensure!(topic["kind"] != "image", "Image topics use visual labeling");
    ensure!(
        topic["errors"].as_array().is_none_or(Vec::is_empty),
        "Resolve decoding errors before converting this topic"
    );
    let frames = topic["frames"].as_array().context("Topic needs frames")?;
    let path = contained_path(
        &recording.cache,
        &format!("frames/channel_{channel}/records.jsonl"),
    )?;
    let mut reader =
        BufReader::new(File::open(path).context("This topic has no decoded message data")?);
    let mut rows = Vec::with_capacity(frames.len());
    for frame in frames {
        let offset = frame["offset"]
            .as_u64()
            .context("Message has no decoded record")?;
        reader.seek(SeekFrom::Start(offset))?;
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let message: Value = serde_json::from_str(&line).context("Invalid decoded message")?;
        let mut fields = Map::new();
        flatten_message(&message, "", &mut fields);
        rows.push(Value::Object(fields));
    }
    Ok(rows)
}

pub(super) fn transform_cell(value: Value, column: &Value) -> Result<Value> {
    if value.is_null() || value == "" {
        return Ok(value);
    }
    match column["transform"].as_str().unwrap_or("identity") {
        "identity" => Ok(value),
        "text" => Ok(json!(value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string()))),
        "number" | "scale" => {
            let number = value
                .as_f64()
                .or_else(|| value.as_str()?.parse::<f64>().ok())
                .context("Value is not numeric")?;
            let scale = column["scale"].as_f64().unwrap_or(1.0);
            let offset = column["offset"].as_f64().unwrap_or(0.0);
            let result = if column["transform"] == "scale" {
                number * scale + offset
            } else {
                number
            };
            ensure!(result.is_finite(), "Numeric result must be finite");
            Ok(json!(result))
        }
        "boolean" => match value
            .as_str()
            .map(|value| value.trim().to_lowercase())
            .as_deref()
        {
            Some("true" | "1" | "yes") => Ok(json!(true)),
            Some("false" | "0" | "no") => Ok(json!(false)),
            _ if value.is_boolean() => Ok(value),
            _ if value == 1 => Ok(json!(true)),
            _ if value == 0 => Ok(json!(false)),
            _ => bail!("Use true/false, yes/no, or 1/0 for boolean columns"),
        },
        _ => bail!("Unknown column transformation"),
    }
}

pub(super) fn csv_cell(value: &Value) -> String {
    let text = if value.is_null() {
        String::new()
    } else {
        value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string())
    };
    format!("\"{}\"", text.replace('"', "\"\""))
}

#[cfg(test)]
mod structured_tests {
    use super::flatten_message;
    use serde_json::{json, Map};
    #[test]
    fn flattening_preserves_nested_arrays_and_escaped_field_paths() {
        let message = json!({"joint":{"angle":2.5},"tags":["a","b"],"a/b":{"~key":true},"empty":{},"stamp":1791057600000000001u64});
        let mut fields = Map::new();
        flatten_message(&message, "", &mut fields);
        assert_eq!(fields["/joint/angle"], 2.5);
        assert_eq!(fields["/tags/1"], "b");
        assert_eq!(fields["/a~1b/~0key"], true);
        assert_eq!(fields["/empty"], json!({}));
        assert_eq!(fields["/stamp"], "1791057600000000001");
    }
}
