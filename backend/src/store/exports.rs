//! Reviewed annotation exports and self-contained ZIP downloads.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    fs::{self, File},
    io::Write,
};
use uuid::Uuid;

use super::{
    canonical_uuid, contained_path, decoded_record, field, indexed_frame, list_files,
    selected_annotation_channels, validate_annotations, write_json, Store,
};

impl Store {
    pub fn export(&self, id: &str, request: &Value) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let recording_id = field(request, "recording_uuid")?;
        let (recording, revision) = if !self.accounts.setup_required()?
            && self.accounts.settings()?.require_approval_for_datasets
        {
            self.latest_approved_recording(id, recording_id, request["channel_id"].as_str())?
        } else {
            (self.recording_for_edit(id, recording_id)?, Value::Null)
        };
        Self::editable(&recording.project)?;
        validate_annotations(
            &recording.index,
            &recording.project,
            &recording.annotations,
            None,
        )?;
        let format = field(request, "format")?;
        ensure!(
            matches!(format, "coco" | "yolo" | "jsonl"),
            "Choose coco, yolo or jsonl"
        );
        let uuid = Uuid::new_v4().to_string();
        let parent = contained_path(&recording.folder, "exports")?;
        fs::create_dir_all(&parent)?;
        let temporary = tempfile::Builder::new()
            .prefix(".export-")
            .tempdir_in(&parent)?;
        let classes = recording.annotations["classes"]
            .as_array()
            .context("Missing classes")?;
        write_json(&temporary.path().join("classes.json"), &json!(classes))?;
        write_json(
            &temporary.path().join("annotations.json"),
            &recording.annotations,
        )?;
        let mut counts = Map::new();
        let mut reviewed_count = 0usize;
        for channel in selected_annotation_channels(&recording, request)? {
            let rows = self.eligible_rows(&recording, &channel)?;
            if rows.is_empty() {
                continue;
            }
            let topic = &recording.index["topics"][&channel];
            ensure!(
                format == "jsonl" || topic["kind"] == "image",
                "COCO and YOLO require image topics"
            );
            let directory = temporary.path().join(format!("channel_{channel}"));
            fs::create_dir_all(directory.join("images"))?;
            fs::create_dir_all(directory.join("labels"))?;
            let mut jsonl = File::create(directory.join("records.jsonl"))?;
            let categories: Vec<Value> = classes
                .iter()
                .enumerate()
                .map(|(i, name)| json!({"id":i,"name":name}))
                .collect();
            let mut coco = json!({"images":[],"annotations":[],"categories":categories});
            for (number, row) in &rows {
                let frame = indexed_frame(&recording.index, &channel, *number)?;
                let mut metadata =
                    json!({"frame":number,"time_ns":frame["time_ns"],"annotation":row});
                let filename = format!("images/{number:08}.png");
                if topic["kind"] == "image" {
                    let image_path = contained_path(
                        &recording.cache,
                        &format!("frames/{}", field(frame, "image")?),
                    )?;
                    fs::copy(image_path, directory.join(&filename))?;
                    metadata["image"] = json!(filename);
                    metadata["width"] = frame["width"].clone();
                    metadata["height"] = frame["height"].clone();
                } else {
                    metadata["record"] = decoded_record(&recording, &channel, *number)?;
                }
                serde_json::to_writer(&mut jsonl, &metadata)?;
                jsonl.write_all(b"\n")?;
                let boxes = row["boxes"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                if format == "coco" {
                    coco["images"].as_array_mut().unwrap().push(json!({"id":number+1,"file_name":filename,"width":frame["width"],"height":frame["height"]}));
                    for bbox in boxes {
                        let annotation_id = coco["annotations"].as_array().unwrap().len() + 1;
                        coco["annotations"].as_array_mut().unwrap().push(json!({"id":annotation_id,"image_id":number+1,"category_id":bbox["class_id"],"bbox":[bbox["x"],bbox["y"],bbox["width"],bbox["height"]],"area":bbox["width"].as_f64().unwrap()*bbox["height"].as_f64().unwrap(),"iscrowd":0}));
                    }
                } else if format == "yolo" {
                    let width = frame["width"].as_f64().context("Missing width")?;
                    let height = frame["height"].as_f64().context("Missing height")?;
                    let mut text = String::new();
                    for bbox in boxes {
                        text.push_str(&format!(
                            "{} {:.10} {:.10} {:.10} {:.10}\n",
                            bbox["class_id"],
                            (bbox["x"].as_f64().unwrap() + bbox["width"].as_f64().unwrap() / 2.0)
                                / width,
                            (bbox["y"].as_f64().unwrap() + bbox["height"].as_f64().unwrap() / 2.0)
                                / height,
                            bbox["width"].as_f64().unwrap() / width,
                            bbox["height"].as_f64().unwrap() / height
                        ));
                    }
                    fs::write(directory.join(format!("labels/{number:08}.txt")), text)?;
                }
            }
            if format == "coco" {
                write_json(&directory.join("annotations.json"), &coco)?;
            }
            if format == "yolo" {
                fs::write(
                    directory.join("data.yaml"),
                    format!(
                        "path: .\ntrain: images\nnames: {}\n",
                        serde_json::to_string(classes)?
                    ),
                )?;
            }
            reviewed_count += rows.len();
            counts.insert(channel, json!(rows.len()));
        }
        ensure!(
            reviewed_count > 0,
            "Approve at least one frame before exporting"
        );
        write_json(
            &temporary.path().join("manifest.json"),
            &json!({"project_uuid":id,"mcap_uuid":recording_id,"mcap_sha256":recording.item["sha256"],"format":format,"counts":counts,"unreviewed_frames":"excluded","approved_revision":revision}),
        )?;
        let destination = parent.join(&uuid);
        fs::rename(temporary.path(), &destination)?;
        let files = list_files(&destination)?;
        Ok(
            json!({"uuid":uuid,"path":format!("exports/{uuid}"),"format":format,"reviewed_count":reviewed_count,"files":files,"download_url":format!("/api/projects/{id}/exports/{uuid}/download")}),
        )
    }

    pub fn export_zip(&self, id: &str, export_id: &str) -> Result<Vec<u8>> {
        canonical_uuid(export_id)?;
        let folder = self.folder(id)?;
        let directory = contained_path(&folder, &format!("exports/{export_id}"))?;
        ensure!(directory.is_dir(), "Export not found");
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for relative in list_files(&directory)? {
            writer.start_file(
                &relative,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            let mut file = File::open(contained_path(&directory, &relative)?)?;
            std::io::copy(&mut file, &mut writer)?;
        }
        Ok(writer.finish()?.into_inner())
    }
}
