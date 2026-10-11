//! Native MCAP pipeline orchestration and output publication.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self},
    io::{BufWriter, Write},
    path::Path,
};

use super::synchronization::{channel_id, resample, timestamp, validate_sync_options, Sample};
use super::{stream_messages, translate_record, Decoder};

#[derive(Clone)]
pub(super) struct Output {
    channel: u16,
    sample: Sample,
}

/// Translate selected channels, optionally resample them onto a parent clock,
/// and write a new real MCAP with JSON payloads and embedded JSON schemas.
pub fn process_recording(source: &Path, destination: &Path, request: &Value) -> Result<Value> {
    ensure!(
        source != destination,
        "Processed output must use a new recording path"
    );
    ensure!(!destination.exists(), "Processed output already exists");
    let selected: Option<BTreeSet<u16>> = request
        .get("selected_channels")
        .map(|value| -> Result<_> {
            let array = value
                .as_array()
                .context("selected_channels must be an array")?;
            ensure!(!array.is_empty(), "Select at least one channel");
            array
                .iter()
                .map(|value| channel_id(value).context("Invalid selected channel ID"))
                .collect()
        })
        .transpose()?;
    let sync = request
        .get("sync")
        .or_else(|| request.get("synchronization"))
        .cloned()
        .unwrap_or_else(|| json!({"enabled": false}));
    let sync_enabled = sync["enabled"].as_bool().unwrap_or(false);
    if sync_enabled {
        validate_sync_options(&sync)?;
    }
    let mut decoder = Decoder::default();
    let mut channels: BTreeMap<u16, Vec<Sample>> = BTreeMap::new();
    let mut metadata: BTreeMap<u16, (String, String, BTreeMap<String, String>)> = BTreeMap::new();
    stream_messages(source, |message| {
        let id = message.channel.id;
        if selected
            .as_ref()
            .is_some_and(|selected| !selected.contains(&id))
        {
            return Ok(());
        }
        let record = decoder
            .decode(message)
            .with_context(|| format!("Channel {id} at {}", message.log_time))?;
        let translation = &request["translations"][id.to_string()];
        let script = translation
            .as_str()
            .or_else(|| translation["script"].as_str())
            .unwrap_or("");
        ensure!(
            translation.is_null() || translation.is_string() || translation["script"].is_string(),
            "Translation for channel {id} must be script text"
        );
        let record = translate_record(&record, script)
            .with_context(|| format!("Translation for channel {id}"))?;
        let options = sync["channels"].get(id.to_string()).unwrap_or(&sync);
        let time = if sync_enabled && options["enabled"] != false {
            timestamp(
                &record,
                options["timestamp_field"]
                    .as_str()
                    .unwrap_or("MCAP log time"),
                message.log_time,
                message.publish_time,
            )?
        } else {
            message.log_time
        };
        metadata.entry(id).or_insert_with(|| {
            (
                message.channel.topic.clone(),
                message
                    .channel
                    .schema
                    .as_ref()
                    .map(|schema| schema.name.clone())
                    .unwrap_or_else(|| "Record".into()),
                message.channel.metadata.clone(),
            )
        });
        let samples = channels.entry(id).or_default();
        samples.push(Sample {
            ordinal: samples.len(),
            time,
            log_time: message.log_time,
            publish_time: message.publish_time,
            sequence: message.sequence,
            record,
        });
        Ok(())
    })?;
    ensure!(!channels.is_empty(), "Selected channels have no messages");
    if let Some(selected) = &selected {
        ensure!(
            selected.iter().all(|id| channels.contains_key(id)),
            "A selected channel is missing or has no messages"
        );
    }
    let input_count: usize = channels.values().map(Vec::len).sum();
    let mut output = Vec::new();
    let mut dropped = 0;
    let mut synchronized = 0;
    let mut misaligned = 0;
    let mut deleted = 0;
    if sync_enabled {
        let parent = channel_id(&sync["parent_channel"])
            .or_else(|| channel_id(&sync["master_channel_id"]))
            .or_else(|| channel_id(&sync["parent_channel_id"]))
            .context("Select a synchronization parent channel")?;
        let parent_samples = channels
            .get(&parent)
            .context("Synchronization parent must be a selected channel with messages")?;
        let delete = sync["delete_out_of_sync"].as_bool().unwrap_or(true);
        let mut samplers = BTreeMap::new();
        for (id, samples) in &channels {
            if *id == parent {
                continue;
            }
            let options = sync["channels"].get(id.to_string()).unwrap_or(&sync);
            if options["enabled"] == false {
                output.extend(samples.iter().cloned().map(|sample| Output {
                    channel: *id,
                    sample,
                }));
                continue;
            }
            // Duplicate source clocks: last message wins, as in the Python reference.
            let mut unique = BTreeMap::new();
            for sample in samples {
                unique.insert(sample.time, sample.clone());
            }
            samplers.insert(*id, unique.into_values().collect::<Vec<_>>());
        }
        let mut used: BTreeMap<u16, BTreeSet<u64>> = BTreeMap::new();
        let mut retained: BTreeMap<u16, BTreeSet<u64>> = BTreeMap::new();
        let mut aligned: BTreeMap<u16, BTreeSet<u64>> = BTreeMap::new();
        let mut parents = parent_samples.clone();
        parents.sort_by_key(|sample| sample.time);
        for parent_sample in parents {
            let mut frame = vec![Output {
                channel: parent,
                sample: Sample {
                    log_time: parent_sample.time,
                    ..parent_sample.clone()
                },
            }];
            let mut contributions = Vec::new();
            let mut missing = false;
            for (id, samples) in &samplers {
                let options = sync["channels"].get(id.to_string()).unwrap_or(&sync);
                if let Some((mut sample, times)) = resample(samples, parent_sample.time, options)? {
                    used.entry(*id).or_default().extend(&times);
                    sample.log_time = parent_sample.time;
                    sample.time = parent_sample.time;
                    frame.push(Output {
                        channel: *id,
                        sample,
                    });
                    contributions.push((*id, times));
                } else {
                    missing = true;
                }
            }
            if missing && delete {
                dropped += 1;
                continue;
            }
            synchronized += frame.len();
            for (id, times) in contributions {
                retained.entry(id).or_default().extend(times);
                aligned.entry(id).or_default().insert(parent_sample.time);
            }
            output.extend(frame);
        }
        for id in samplers.keys() {
            let all = &channels[id];
            let unique = &samplers[id];
            for sample in all {
                let is_last = unique
                    .binary_search_by_key(&sample.time, |s| s.time)
                    .ok()
                    .is_some_and(|i| unique[i].ordinal == sample.ordinal);
                if !is_last
                    || !aligned
                        .get(id)
                        .is_some_and(|times| times.contains(&sample.time))
                {
                    misaligned += 1;
                }
                if delete
                    && (!is_last
                        || !retained
                            .get(id)
                            .is_some_and(|times| times.contains(&sample.time)))
                {
                    deleted += 1;
                }
                if !delete
                    && !used
                        .get(id)
                        .is_some_and(|times| times.contains(&sample.time))
                {
                    output.push(Output {
                        channel: *id,
                        sample: sample.clone(),
                    });
                }
            }
        }
        deleted += dropped;
    } else {
        for (id, samples) in &channels {
            output.extend(samples.iter().cloned().map(|sample| Output {
                channel: *id,
                sample,
            }));
        }
    }
    output.sort_by_key(|item| (item.sample.log_time, item.channel, item.sample.sequence));
    let directory = destination
        .parent()
        .context("Output has no parent directory")?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    let mut writer = mcap::Writer::new(BufWriter::new(temporary.as_file_mut()))?;
    let mut written_channels = BTreeMap::new();
    for (id, (topic, schema_name, metadata)) in &metadata {
        let schema = serde_json::to_vec(
            &json!({"$schema": "https://json-schema.org/draft/2020-12/schema", "title": schema_name, "type": "object", "additionalProperties": true}),
        )?;
        let schema_id = writer.add_schema(
            &format!("{schema_name}.TrainingRecord"),
            "jsonschema",
            &schema,
        )?;
        let mut metadata = metadata.clone();
        metadata.insert("source_channel_id".into(), id.to_string());
        metadata.insert("processing_engine".into(), "mcap-workbench-rust".into());
        let technique = &request["techniques"][id.to_string()];
        if let Some(technique) = technique.as_str() {
            metadata.insert("labeling_technique".into(), technique.into());
        }
        written_channels.insert(
            *id,
            writer.add_channel(schema_id, topic, "json", &metadata)?,
        );
    }
    let mut counts = Map::new();
    for item in &output {
        let id = written_channels[&item.channel];
        writer.write_to_known_channel(
            &mcap::records::MessageHeader {
                channel_id: id,
                sequence: item.sample.sequence,
                log_time: item.sample.log_time,
                publish_time: item.sample.publish_time,
            },
            &serde_json::to_vec(&item.sample.record)?,
        )?;
        let key = item.channel.to_string();
        let count = counts.get(&key).and_then(Value::as_u64).unwrap_or(0) + 1;
        counts.insert(key, json!(count));
    }
    writer.finish()?;
    let mut stream = writer.into_inner();
    stream.flush()?;
    drop(stream);
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(
        json!({"input_messages": input_count, "output_messages": output.len(), "channel_counts": counts, "channel_id_map": written_channels,
        "dropped_frames": dropped, "message_counts": {"misaligned": misaligned, "synchronized": synchronized, "deleted": deleted},
        "output_encoding": "json", "synchronization_enabled": sync_enabled}),
    )
}
