//! Annotation mutations, optimistic conflicts, geometry and class validation.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

use super::{contained_path, field, indexed_frame, same_json, Recording, Store};

impl Store {
    pub fn save_annotations(&self, id: &str, recording_id: &str, request: &Value) -> Result<Value> {
        self.save_annotations_inner(id, recording_id, request, None)
    }
    pub fn save_annotations_as(
        &self,
        id: &str,
        recording_id: &str,
        request: &Value,
        actor: &crate::accounts::Principal,
    ) -> Result<Value> {
        self.save_annotations_inner(id, recording_id, request, Some(actor))
    }
    fn save_annotations_inner(
        &self,
        id: &str,
        recording_id: &str,
        request: &Value,
        actor: Option<&crate::accounts::Principal>,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let recording = self.recording_for_edit(id, recording_id)?;
        let (changes, metadata) = annotation_changes(&recording.annotations, request)?;
        let tasks = actor
            .map(|a| self.authorize_annotation_changes(&recording, a, &changes, &metadata))
            .transpose()?;
        let saved = self.save_annotation_state(&recording, recording_id, request)?;
        if let Some(actor) = actor {
            self.annotation_change_audit(&recording, actor, tasks.unwrap_or_default(), &changes)?;
        }
        Ok(saved)
    }

    /// Apply only changed rows. Expected values prevent overwriting another editor.
    pub fn patch_annotations(
        &self,
        id: &str,
        recording_id: &str,
        request: &Value,
    ) -> Result<Value> {
        self.patch_annotations_inner(id, recording_id, request, None)
    }
    pub fn patch_annotations_as(
        &self,
        id: &str,
        recording_id: &str,
        request: &Value,
        actor: &crate::accounts::Principal,
    ) -> Result<Value> {
        self.patch_annotations_inner(id, recording_id, request, Some(actor))
    }
    fn patch_annotations_inner(
        &self,
        id: &str,
        recording_id: &str,
        request: &Value,
        actor: Option<&crate::accounts::Principal>,
    ) -> Result<Value> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Store unavailable"))?;
        let folder = self.folder(id)?;
        let _lock = Self::lock_project(&folder)?;
        let compact = request["return_delta"] == true;
        let buckets = crate::storage::annotation_buckets(&request["changes"])?;
        let subset = compact
            && request["metadata"].as_object().is_none_or(|metadata| {
                metadata
                    .keys()
                    .all(|key| matches!(key.as_str(), "settings" | "tracks"))
            });
        let recording =
            self.read_recording_with_pages(id, recording_id, true, subset.then_some(&buckets))?;
        ensure!(
            request["sha256"] == recording.item["sha256"],
            "Annotation SHA-256 does not match recording"
        );
        let tasks = actor
            .map(|a| {
                self.authorize_annotation_changes(
                    &recording,
                    a,
                    &request["changes"],
                    &request["metadata"],
                )
            })
            .transpose()?;
        let mut state = recording.annotations.clone();
        for change in request["changes"]
            .as_array()
            .context("Provide annotation changes")?
        {
            let channel = field(change, "channel_id")?;
            let number = change["frame"]
                .as_u64()
                .context("Frame must be a nonnegative integer")?
                .to_string();
            indexed_frame(&recording.index, channel, number.parse()?)?;
            if let Some(expected) = change.get("expected") {
                ensure!(
                    same_json(&state["annotations"][channel][&number], expected),
                    "Annotation changed; refresh before saving"
                );
            }
            let row = change
                .get("annotation")
                .context("Provide annotation or null")?;
            if same_json(&state["annotations"][channel][&number], row) {
                continue;
            }
            if !state["annotations"][channel].is_object() {
                state["annotations"][channel] = json!({});
            }
            if row.is_null() {
                state["annotations"][channel]
                    .as_object_mut()
                    .unwrap()
                    .remove(&number);
            } else {
                state["annotations"][channel][&number] = row.clone();
            }
        }
        if let Some(metadata) = request.get("metadata") {
            for (key, value) in metadata.as_object().context("Metadata must be an object")? {
                ensure!(
                    !["annotations", "sha256", "format_version", "pages"].contains(&key.as_str()),
                    "Cannot patch reserved annotation metadata"
                );
                if let Some(expected) = request["expected_metadata"].get(key) {
                    ensure!(
                        same_json(&recording.annotations[key], expected),
                        "Annotation settings changed; refresh before saving"
                    );
                }
                state[key] = value.clone();
            }
        }
        if !compact {
            let saved = self.save_annotation_state(&recording, recording_id, &state)?;
            if let Some(actor) = actor {
                self.annotation_change_audit(
                    &recording,
                    actor,
                    tasks.unwrap_or_default(),
                    &request["changes"],
                )?;
            }
            return Ok(saved);
        }
        let state = self.validated_annotation_state(&recording, &state)?;
        if recording.project["format_version"] == 3 {
            crate::storage::save_annotation_subset(&folder, recording_id, &state, &buckets)?;
        } else {
            let path = contained_path(&folder, field(&recording.item, "path")?)?;
            crate::storage::write_changed(
                &contained_path(path.parent().unwrap(), "annotations.json")?,
                &state,
            )?;
        }
        if let Some(actor) = actor {
            self.annotation_change_audit(
                &recording,
                actor,
                tasks.unwrap_or_default(),
                &request["changes"],
            )?;
        }
        let mut metadata = state;
        metadata.as_object_mut().unwrap().remove("annotations");
        Ok(
            json!({"sha256":recording.item["sha256"],"metadata":metadata,"changes":request["changes"]}),
        )
    }

    pub(super) fn save_annotation_state(
        &self,
        recording: &Recording,
        recording_id: &str,
        request: &Value,
    ) -> Result<Value> {
        let folder = &recording.folder;
        let state = self.validated_annotation_state(recording, request)?;
        let recording_path = contained_path(folder, field(&recording.item, "path")?)?;
        if recording.project["format_version"] == 3 {
            crate::storage::save_annotations(folder, recording_id, &state)?;
        } else {
            crate::storage::write_changed(
                &contained_path(recording_path.parent().unwrap(), "annotations.json")?,
                &state,
            )?;
        }
        Ok(state)
    }

    pub(super) fn validated_annotation_state(
        &self,
        recording: &Recording,
        request: &Value,
    ) -> Result<Value> {
        Self::editable(&recording.project)?;
        let mut state = request.clone();
        // Browser JSON can spell an unchanged float as an integer. Preserve the
        // stored representation so a no-op save cannot change a review digest.
        if let Some(channels) = state["annotations"].as_object_mut() {
            for (channel, rows) in channels {
                for (number, row) in rows.as_object_mut().into_iter().flatten() {
                    let previous = &recording.annotations["annotations"][channel][number];
                    if same_json(row, previous) {
                        *row = previous.clone();
                    }
                }
            }
        }
        for (key, value) in state
            .as_object_mut()
            .context("Annotation state must be an object")?
        {
            if key != "annotations" && same_json(value, &recording.annotations[key]) {
                *value = recording.annotations[key].clone();
            }
        }
        for (channel, technique) in state["techniques"]
            .as_object()
            .context("Provide techniques")?
        {
            if recording.annotations["techniques"][channel] != *technique {
                ensure!(matches!(technique.as_str(), Some("undefined" | "yolo" | "csv" | "classification")), "Choose YOLO for images or CSV for message data; classification is available for video/images");
            }
        }
        if state.get("tracks").is_none() {
            if let Some(tracks) = recording.annotations.get("tracks") {
                state["tracks"] = tracks.clone();
            }
        }
        validate_annotations(
            &recording.index,
            &recording.project,
            &state,
            Some(&recording.annotations),
        )?;
        refresh_classes(&recording.project, &mut state)?;
        Ok(state)
    }
}

fn annotation_changes(previous: &Value, next: &Value) -> Result<(Value, Value)> {
    let mut changes = vec![];
    let channels: std::collections::BTreeSet<_> = previous["annotations"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k)
        .chain(
            next["annotations"]
                .as_object()
                .context("Provide annotations")?
                .keys(),
        )
        .collect();
    for channel in channels {
        let numbers: std::collections::BTreeSet<_> = previous["annotations"][channel]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k)
            .chain(
                next["annotations"][channel]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(k, _)| k),
            )
            .collect();
        for number in numbers {
            let row = &next["annotations"][channel][number];
            if !same_json(row, &previous["annotations"][channel][number]) {
                changes.push(
                    json!({"channel_id":channel,"frame":number.parse::<u64>()?,"annotation":row}),
                );
            }
        }
    }
    let mut metadata = next
        .as_object()
        .context("Provide annotation state")?
        .clone();
    for key in ["annotations", "sha256", "format_version"] {
        metadata.remove(key);
    }
    Ok((json!(changes), json!(metadata)))
}

pub(super) fn refresh_classes(project: &Value, state: &mut Value) -> Result<()> {
    let mut classes = state["classes"]
        .as_array()
        .context("Annotations need classes")?
        .clone();
    let mut ids = state["class_uuids"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![Value::Null; classes.len()]);
    ensure!(
        ids.len() <= classes.len(),
        "Class UUIDs do not match numeric classes"
    );
    ids.resize(classes.len(), Value::Null);
    for (id, label) in project["labels"].as_object().into_iter().flatten() {
        if let Some(position) = ids.iter().position(|saved| saved == id) {
            classes[position] = label["name"].clone();
        } else if let Some(position) = classes.iter().position(|saved| saved == &label["name"]) {
            ids[position] = json!(id);
        } else {
            classes.push(label["name"].clone());
            ids.push(json!(id));
        }
    }
    state["classes"] = json!(classes);
    state["class_uuids"] = json!(ids);
    Ok(())
}

pub(super) const BOX_TECHNIQUES: &[&str] = &["yolo", "coco", "pascal_voc", "kitti", "mot", "cvat"];

pub(super) fn validate_box(
    index: &Value,
    classes: &[Value],
    channel: &str,
    number: usize,
    bbox: &Value,
) -> Result<()> {
    let frame = indexed_frame(index, channel, number)?;
    ensure!(
        bbox["class_id"]
            .as_u64()
            .is_some_and(|id| id < classes.len() as u64),
        "Box has invalid numeric class ID"
    );
    ensure!(
        bbox["track_id"].as_u64().is_some_and(|id| id > 0),
        "Box track ID must be a positive integer"
    );
    let coordinate = |key: &str| -> Result<f64> {
        let value = bbox[key]
            .as_f64()
            .with_context(|| format!("Box {key} must be numeric"))?;
        ensure!(value.is_finite(), "Box coordinates must be finite");
        Ok(value)
    };
    let x = coordinate("x")?;
    let y = coordinate("y")?;
    let width = coordinate("width")?;
    let height = coordinate("height")?;
    let frame_width = frame["width"]
        .as_f64()
        .context("Bounding boxes require image frames")?;
    let frame_height = frame["height"]
        .as_f64()
        .context("Bounding boxes require image frames")?;
    ensure!(
        x >= 0.0
            && y >= 0.0
            && width > 0.0
            && height > 0.0
            && x + width <= frame_width + 1e-6
            && y + height <= frame_height + 1e-6,
        "Box must fit inside the frame and have positive size"
    );
    Ok(())
}

pub fn validate_annotations(
    index: &Value,
    project: &Value,
    state: &Value,
    previous: Option<&Value>,
) -> Result<()> {
    ensure!(
        state["format_version"] == 1 && state["sha256"] == index["sha256"],
        "Annotations SHA-256 does not match recording"
    );
    let classes = state["classes"]
        .as_array()
        .context("Provide classes as an array")?;
    let mut names = HashSet::new();
    for class in classes {
        let name = class.as_str().context("Class names must be strings")?;
        ensure!(
            !name.trim().is_empty() && names.insert(name),
            "Class names must be unique and nonempty"
        );
    }
    let techniques = state["techniques"]
        .as_object()
        .context("Provide techniques")?;
    for (channel, technique) in techniques {
        let topic = index["topics"]
            .get(channel)
            .context("Technique references unknown topic")?;
        let technique = technique.as_str().context("Technique must be a string")?;
        ensure!(
            technique == "undefined"
                || technique == "classification"
                || technique == "jsonl"
                || technique == "csv"
                || BOX_TECHNIQUES.contains(&technique),
            "Unsupported labeling technique"
        );
        if technique == "classification"
            && previous.is_none_or(|old| old["techniques"][channel] != "classification")
        {
            ensure!(
                topic["kind"] == "image",
                "Classification requires video/image topics"
            );
        }
        if technique == "csv" {
            ensure!(
                topic["kind"] != "image",
                "CSV requires structured message topics"
            );
        }
        if BOX_TECHNIQUES.contains(&technique) {
            ensure!(
                topic["kind"] == "image",
                "Bounding box techniques require image topics"
            );
        }
    }
    let annotations = state["annotations"]
        .as_object()
        .context("Provide annotations as an object")?;
    for (channel, rows) in annotations {
        let topic = index["topics"]
            .get(channel)
            .context("Annotations reference unknown topic")?;
        ensure!(
            topic["errors"].as_array().is_none_or(Vec::is_empty),
            "Resolve decoding errors before annotating this topic"
        );
        for (number, row) in rows
            .as_object()
            .context("Frame annotations must be an object")?
        {
            let number: usize = number.parse().context("Frame ID must be numeric")?;
            indexed_frame(index, channel, number)?;
            ensure!(row["reviewed"].is_boolean(), "Reviewed must be boolean");
            let mut tracks = HashSet::new();
            for bbox in row["boxes"]
                .as_array()
                .context("Frame boxes must be an array")?
            {
                validate_box(index, classes, channel, number, bbox)?;
                ensure!(
                    tracks.insert(bbox["track_id"].as_u64().unwrap()),
                    "A track may have only one box per frame"
                );
            }
            for label in row["labels"]
                .as_array()
                .context("Frame labels must be an array")?
            {
                ensure!(
                    label.as_u64().is_some_and(|id| id < classes.len() as u64),
                    "Invalid classification label"
                );
            }
        }
    }
    if let Some(previous) = previous {
        for channel in index["topics"]
            .as_object()
            .context("Index needs topics")?
            .keys()
        {
            let existing = previous["annotations"][channel]
                .as_object()
                .is_some_and(|rows| !rows.is_empty())
                || previous["tracks"][channel]
                    .as_object()
                    .is_some_and(|rows| !rows.is_empty());
            if existing {
                let old = previous["techniques"][channel]
                    .as_str()
                    .unwrap_or("undefined");
                let new = state["techniques"][channel].as_str().unwrap_or("undefined");
                ensure!(
                    old == new || (BOX_TECHNIQUES.contains(&old) && BOX_TECHNIQUES.contains(&new)),
                    "Keep the saved labeling technique for topics with existing annotations"
                );
            }
        }
        for id in used_class_ids(previous, None) {
            ensure!(
                classes.get(id) == previous["classes"].get(id),
                "Preserve numeric IDs and names of classes already used"
            );
            if let Some(previous_id) = previous["class_uuids"].get(id).filter(|id| !id.is_null()) {
                ensure!(
                    state["class_uuids"].get(id) == Some(previous_id),
                    "Preserve UUID mappings of classes already used"
                );
            }
        }
    }
    if let Some(ids) = state["class_uuids"].as_array() {
        ensure!(
            ids.len() == classes.len(),
            "Class UUIDs must align with numeric classes"
        );
        let mut unique = HashSet::new();
        for (number, id) in ids.iter().enumerate() {
            ensure!(
                id.is_null()
                    || id
                        .as_str()
                        .is_some_and(|id| project["labels"].get(id).is_some()),
                "Class UUID references unknown project label"
            );
            if let Some(id) = id.as_str() {
                ensure!(
                    unique.insert(id) && project["labels"][id]["name"] == classes[number],
                    "Class UUIDs must be unique and match their numeric class names"
                );
            }
        }
    }
    for (channel, ids) in state["topic_labels"].as_object().into_iter().flatten() {
        ensure!(
            index["topics"].get(channel).is_some(),
            "Topic labels reference unknown topic"
        );
        for id in ids.as_array().context("Topic labels must be UUID arrays")? {
            ensure!(
                id.as_str()
                    .is_some_and(|id| project["labels"].get(id).is_some()),
                "Topic references unknown project label"
            );
        }
    }
    for (channel, id) in state["topic_label_lists"].as_object().into_iter().flatten() {
        ensure!(
            index["topics"].get(channel).is_some()
                && id
                    .as_str()
                    .is_some_and(|id| project["label_lists"].get(id).is_some()),
            "Topic references unknown label list"
        );
    }
    for (channel, tracks) in state["tracks"].as_object().into_iter().flatten() {
        for (id, track) in tracks.as_object().context("Tracks must be an object")? {
            ensure!(
                id.parse::<u64>().is_ok_and(|id| id > 0),
                "Track ID must be a positive integer"
            );
            let frame_count = index["topics"]
                .get(channel)
                .context("Track references unknown topic")?["frames"]
                .as_array()
                .context("Track topic needs frames")?
                .len();
            let start = track["start"]
                .as_u64()
                .context("Track start must be a nonnegative frame")?;
            ensure!(
                start < frame_count as u64,
                "Track start is outside this topic"
            );
            let end = if track["end"].is_null() {
                None
            } else {
                Some(
                    track["end"]
                        .as_u64()
                        .context("Track end must be a nonnegative frame or null")?,
                )
            };
            ensure!(
                end.is_none_or(|end| end >= start && end < frame_count as u64),
                "Track end must follow its start and fit inside the topic"
            );
            let keyframes = track["keyframes"]
                .as_object()
                .context("Track needs keyframes")?;
            ensure!(
                !keyframes.is_empty(),
                "Track must contain at least one keyframe"
            );
            for (number, bbox) in keyframes {
                let number: usize = number.parse()?;
                ensure!(
                    number as u64 >= start && end.is_none_or(|end| number as u64 <= end),
                    "Track keyframes must fit inside its lifecycle"
                );
                validate_box(index, classes, channel, number, bbox)?;
                ensure!(
                    bbox["track_id"].as_u64() == id.parse().ok(),
                    "Track keyframe ID does not match track"
                );
            }
        }
    }
    for channel in index["topics"]
        .as_object()
        .context("Index needs topics")?
        .keys()
    {
        let allowed = state["topic_label_lists"][channel]
            .as_str()
            .map(|id| &project["label_lists"][id]["label_ids"])
            .or_else(|| state["topic_labels"].get(channel));
        if let Some(allowed) = allowed {
            let allowed = allowed
                .as_array()
                .context("Topic labels must be UUID arrays")?;
            let retired = state["topic_label_lists"][channel]
                .as_str()
                .and_then(|list_id| {
                    project["label_lists"][list_id]["retired_label_ids"].as_array()
                });
            for id in used_class_ids(state, Some(channel)) {
                let class_id = state["class_uuids"]
                    .get(id)
                    .context("Associated labels require class UUIDs")?;
                ensure!(
                    !class_id.is_null()
                        && (allowed.contains(class_id)
                            || retired.is_some_and(|ids| ids.contains(class_id))),
                    "Keep associated labels already used by this topic"
                );
            }
        }
    }
    Ok(())
}

pub(super) fn used_class_ids(state: &Value, channel: Option<&str>) -> HashSet<usize> {
    let mut used = HashSet::new();
    for (key, rows) in state["annotations"].as_object().into_iter().flatten() {
        if channel.is_some_and(|channel| channel != key) {
            continue;
        }
        for row in rows.as_object().into_iter().flat_map(|rows| rows.values()) {
            for bbox in row["boxes"].as_array().into_iter().flatten() {
                if let Some(id) = bbox["class_id"].as_u64() {
                    used.insert(id as usize);
                }
            }
            for label in row["labels"].as_array().into_iter().flatten() {
                if let Some(id) = label.as_u64() {
                    used.insert(id as usize);
                }
            }
        }
    }
    for (key, tracks) in state["tracks"].as_object().into_iter().flatten() {
        if channel.is_some_and(|channel| channel != key) {
            continue;
        }
        for track in tracks
            .as_object()
            .into_iter()
            .flat_map(|tracks| tracks.values())
        {
            for bbox in track["keyframes"]
                .as_object()
                .into_iter()
                .flat_map(|frames| frames.values())
            {
                if let Some(id) = bbox["class_id"].as_u64() {
                    used.insert(id as usize);
                }
            }
        }
    }
    used
}

pub(super) fn selected_annotation_channels(
    recording: &Recording,
    request: &Value,
) -> Result<Vec<String>> {
    if let Some(channel) = request["channel_id"].as_str() {
        ensure!(
            recording.index["topics"].get(channel).is_some(),
            "Topic not found"
        );
        Ok(vec![channel.to_owned()])
    } else {
        Ok(recording.index["topics"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect())
    }
}

pub(super) fn reviewed_rows<'a>(
    recording: &'a Recording,
    channel: &str,
) -> Result<Vec<(usize, &'a Value)>> {
    let mut rows = Vec::new();
    for (number, row) in recording.annotations["annotations"][channel]
        .as_object()
        .into_iter()
        .flatten()
    {
        if row["reviewed"] == true && row["skipped"] != true {
            ensure!(
                recording.annotations["techniques"][channel] != "undefined"
                    && !recording.annotations["techniques"][channel].is_null(),
                "Choose a labeling technique before adding reviewed data"
            );
            rows.push((number.parse()?, row));
        }
    }
    rows.sort_by_key(|(number, _)| *number);
    Ok(rows)
}
