//! Bounded-memory topic translations. Untouched channels retain their native payloads.
use super::{stream_messages, translation::CompiledTranslation, Decoder};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufWriter, Seek, Write},
    path::Path,
};

fn add_channel<W: Write + Seek>(
    writer: &mut mcap::Writer<W>,
    channel: &mcap::Channel<'_>,
    translated: bool,
    request: &Value,
) -> Result<u16> {
    let mut metadata = channel.metadata.clone();
    metadata.insert("source_channel_id".into(), channel.id.to_string());
    metadata.insert("processing_engine".into(), "mcap-workbench-rust".into());
    if let Some(technique) = request["techniques"][channel.id.to_string()].as_str() {
        metadata.insert("labeling_technique".into(), technique.into());
    }
    let schema_id = if translated {
        let name = channel
            .schema
            .as_ref()
            .map(|schema| schema.name.as_str())
            .unwrap_or("Record");
        writer.add_schema(
            &format!("{name}.TrainingRecord"),
            "jsonschema",
            &serde_json::to_vec(&json!({"type":"object","additionalProperties":true}))?,
        )?
    } else if let Some(schema) = &channel.schema {
        writer.add_schema(&schema.name, &schema.encoding, &schema.data)?
    } else {
        0
    };
    Ok(writer.add_channel(
        schema_id,
        &channel.topic,
        if translated {
            "json"
        } else {
            &channel.message_encoding
        },
        &metadata,
    )?)
}

pub(super) fn process(
    source: &Path,
    destination: &Path,
    request: &Value,
    selected: Option<&BTreeSet<u16>>,
) -> Result<Value> {
    let preserve = request["preserve_untranslated"] == true;
    let mut scripts = BTreeMap::new();
    if let Some(translations) = request["translations"].as_object().filter(|_| preserve) {
        for (channel, translation) in translations {
            let channel = channel
                .parse::<u16>()
                .context("Invalid translation channel")?;
            ensure!(
                selected.is_none_or(|selected| selected.contains(&channel)),
                "Translated topics must be selected"
            );
            let script = translation
                .as_str()
                .or_else(|| translation["script"].as_str())
                .context("Translations require script text")?;
            scripts.insert(channel, CompiledTranslation::parse(script)?);
        }
    }
    let directory = destination
        .parent()
        .context("Output has no parent directory")?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    let mut writer = mcap::Writer::new(BufWriter::new(temporary.as_file_mut()))?;
    let mut decoder = Decoder::default();
    let mut written = BTreeMap::new();
    let mut counts = Map::new();
    let mut total = 0u64;
    let declared = stream_messages(source, |message| {
        let id = message.channel.id;
        if selected.is_some_and(|selected| !selected.contains(&id)) {
            return Ok(());
        }
        if !preserve {
            if let std::collections::btree_map::Entry::Vacant(entry) = scripts.entry(id) {
                let translation = &request["translations"][id.to_string()];
                let script = translation
                    .as_str()
                    .or_else(|| translation["script"].as_str());
                ensure!(
                    translation.is_null() || script.is_some(),
                    "Translation for channel {id} must be script text"
                );
                entry.insert(CompiledTranslation::parse(script.unwrap_or(""))?);
            }
        }
        let output_id = if let Some(output_id) = written.get(&id) {
            *output_id
        } else {
            let output_id = add_channel(
                &mut writer,
                &message.channel,
                !preserve || scripts.contains_key(&id),
                request,
            )?;
            written.insert(id, output_id);
            output_id
        };
        let header = mcap::records::MessageHeader {
            channel_id: output_id,
            sequence: message.sequence,
            log_time: message.log_time,
            publish_time: message.publish_time,
        };
        if let Some(script) = scripts.get(&id) {
            let record = decoder
                .decode(message)
                .with_context(|| format!("Channel {id} at {}", message.log_time))?;
            let output = script
                .apply_owned(record)
                .with_context(|| format!("Translation for channel {id} at {}", message.log_time))?;
            writer.write_to_known_channel(&header, &serde_json::to_vec(&output)?)?;
        } else {
            writer.write_to_known_channel(&header, &message.data)?;
        }
        let key = id.to_string();
        let count = counts.get(&key).and_then(Value::as_u64).unwrap_or(0) + 1;
        counts.insert(key, json!(count));
        total += 1;
        Ok(())
    })?;
    ensure!(total > 0, "Selected channels have no messages");
    if let Some(selected) = selected {
        ensure!(
            selected.iter().all(|id| {
                declared.contains_key(id) && (preserve || counts.contains_key(&id.to_string()))
            }),
            "A selected channel is missing or has no messages"
        );
    }
    ensure!(
        !preserve
            || scripts
                .keys()
                .all(|id| counts.contains_key(&id.to_string())),
        "A translated channel has no messages"
    );
    // Retain declared empty channels as well as opaque or unsupported streams.
    for (id, channel) in declared {
        if preserve
            && !written.contains_key(&id)
            && selected.is_none_or(|selected| selected.contains(&id))
        {
            written.insert(id, add_channel(&mut writer, &channel, false, request)?);
        }
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
        json!({"input_messages":total,"output_messages":total,"channel_counts":counts,"channel_id_map":written,"dropped_frames":0,"message_counts":{"misaligned":0,"synchronized":0,"deleted":0},"output_encoding":if preserve { "mixed" } else { "json" },"synchronization_enabled":false}),
    )
}
