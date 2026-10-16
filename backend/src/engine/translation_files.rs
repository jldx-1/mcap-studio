//! Schema identities compatible with legacy version-bound translation files.
use super::{stream_messages, Decoder};
use anyhow::{bail, Context, Result};
use prost::Message;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read, Seek},
    path::Path,
};

pub(super) fn schema_fingerprint(channel: &mcap::Channel<'_>) -> Result<String> {
    let bytes = if let Some(schema) = &channel.schema {
        if schema.encoding == "protobuf" {
            let mut descriptors = prost_types::FileDescriptorSet::decode(schema.data.as_ref())?;
            descriptors.file.sort_by(|a, b| a.name.cmp(&b.name));
            descriptors.encode_to_vec()
        } else {
            schema.data.to_vec()
        }
    } else {
        channel.message_encoding.as_bytes().to_vec()
    };
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn translation_channels(path: &Path) -> Result<Value> {
    use mcap::sans_io::{SummaryReadEvent, SummaryReader, SummaryReaderOptions};
    let mut file = BufReader::new(File::open(path)?);
    let mut reader = SummaryReader::new_with_options(
        SummaryReaderOptions::default().with_file_size(file.get_ref().metadata()?.len()),
    );
    while let Some(event) = reader.next_event() {
        match event? {
            SummaryReadEvent::ReadRequest(need) => {
                let count = file.read(reader.insert(need))?;
                reader.notify_read(count);
            }
            SummaryReadEvent::SeekRequest(to) => {
                let position = file.seek(to)?;
                reader.notify_seeked(position);
            }
        }
    }
    let channels = if let Some(summary) = reader
        .finish()
        .filter(|summary| !summary.channels.is_empty())
    {
        summary.channels.into_iter().collect::<BTreeMap<_, _>>()
    } else {
        stream_messages(path, |_| Ok(()))?
    };
    let topics: Vec<_> = channels.values().map(|channel| -> Result<Value> {
        Ok(json!({"channel_id":channel.id.to_string(),"topic":channel.topic,
            "source_message":channel.schema.as_ref().map(|schema|schema.name.as_str()).unwrap_or(""),
            "source_schema_sha256":schema_fingerprint(channel)?,"encoding":channel.message_encoding}))
    }).collect::<Result<_>>()?;
    Ok(json!(topics))
}

pub fn translation_sample(path: &Path, channel: u16) -> Result<Value> {
    let mut decoder = Decoder::default();
    let mut sample = None;
    let result = stream_messages(path, |message| {
        if message.channel.id == channel {
            sample = Some(
                decoder
                    .decode(message)
                    .context("Cannot decode a sample for this topic")?,
            );
            // Stop streaming after the first sample; the source file is read-only.
            bail!("Translation sample collected");
        }
        Ok(())
    });
    if let Some(sample) = sample {
        return Ok(sample);
    }
    result?;
    bail!("This topic has no messages to preview")
}

pub fn translation_file_script(value: &Value) -> Result<String> {
    if let Some(script) = value["script"].as_str() {
        return Ok(script.into());
    }
    let mappings = value["mappings"]
        .as_array()
        .context("Translation file needs script text or mappings")?;
    let lines: Vec<_> = mappings
        .iter()
        .map(|mapping| -> Result<String> {
            Ok(format!(
                "{} <- {}",
                mapping["target"]
                    .as_str()
                    .context("Mapping needs a target")?,
                mapping["expression"]
                    .as_str()
                    .context("Mapping needs an expression")?
            ))
        })
        .collect::<Result<_>>()?;
    Ok(lines.join("\n"))
}
