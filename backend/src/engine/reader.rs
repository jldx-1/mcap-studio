//! Streaming MCAP records and schema/channel identity checks.
use anyhow::{ensure, Context, Result};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read},
    path::Path,
    sync::Arc,
};

/// Stream chunks instead of allocating a buffer the size of the recording.
pub(super) fn stream_messages(
    path: &Path,
    mut visit: impl FnMut(&mcap::Message<'_>) -> Result<()>,
) -> Result<BTreeMap<u16, Arc<mcap::Channel<'static>>>> {
    use mcap::{
        records::Record,
        sans_io::{LinearReadEvent, LinearReader, LinearReaderOptions},
    };
    let mut file = BufReader::new(File::open(path)?);
    let mut reader = LinearReader::new_with_options(
        LinearReaderOptions::default().with_validate_chunk_crcs(true),
    );
    let mut schemas: BTreeMap<u16, Arc<mcap::Schema<'static>>> = BTreeMap::new();
    let mut channels: BTreeMap<u16, Arc<mcap::Channel<'static>>> = BTreeMap::new();
    while let Some(event) = reader.next_event() {
        match event? {
            LinearReadEvent::ReadRequest(need) => {
                let count = file.read(reader.insert(need))?;
                reader.notify_read(count);
            }
            LinearReadEvent::Record { opcode, data } => match mcap::parse_record(opcode, data)? {
                Record::Schema { header, data } => {
                    ensure!(header.id != 0, "Invalid schema ID");
                    let schema = Arc::new(mcap::Schema {
                        id: header.id,
                        name: header.name,
                        encoding: header.encoding,
                        data: Cow::Owned(data.into_owned()),
                    });
                    ensure!(
                        schemas.get(&schema.id).is_none_or(|old| **old == *schema),
                        "Conflicting MCAP schemas"
                    );
                    schemas.insert(schema.id, schema);
                }
                Record::Channel(channel) => {
                    let schema = if channel.schema_id == 0 {
                        None
                    } else {
                        Some(
                            schemas
                                .get(&channel.schema_id)
                                .context("Unknown MCAP schema")?
                                .clone(),
                        )
                    };
                    let channel = Arc::new(mcap::Channel {
                        id: channel.id,
                        topic: channel.topic,
                        schema,
                        message_encoding: channel.message_encoding,
                        metadata: channel.metadata,
                    });
                    ensure!(
                        channels
                            .get(&channel.id)
                            .is_none_or(|old| **old == *channel),
                        "Conflicting MCAP channels"
                    );
                    channels.insert(channel.id, channel);
                }
                Record::Message { header, data } => {
                    let channel = channels
                        .get(&header.channel_id)
                        .context("Unknown MCAP channel")?
                        .clone();
                    visit(&mcap::Message {
                        channel,
                        sequence: header.sequence,
                        log_time: header.log_time,
                        publish_time: header.publish_time,
                        data,
                    })?;
                }
                _ => {}
            },
        }
    }
    Ok(channels)
}
