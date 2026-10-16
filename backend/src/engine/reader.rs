//! Streaming MCAP records and schema/channel identity checks.
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read},
    path::Path,
    sync::Arc,
};

type Channels = BTreeMap<u16, Arc<mcap::Channel<'static>>>;

/// Stream chunks instead of allocating a buffer the size of the recording.
pub(super) fn stream_messages(
    path: &Path,
    visit: impl FnMut(&mcap::Message<'_>) -> Result<()>,
) -> Result<Channels> {
    stream_messages_with_hash(path, false, visit).map(|(channels, _)| channels)
}

/// Optionally digest the source during its existing sequential indexing read.
pub(super) fn stream_messages_with_hash(
    path: &Path,
    hash: bool,
    mut visit: impl FnMut(&mcap::Message<'_>) -> Result<()>,
) -> Result<(Channels, Option<String>)> {
    let mut digest = hash.then(Sha256::new);
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
                let buffer = reader.insert(need);
                let count = file.read(buffer)?;
                if let Some(digest) = &mut digest {
                    digest.update(&buffer[..count]);
                }
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
    // Include any bytes following the MCAP end magic in the file identity too.
    if let Some(digest) = &mut digest {
        let mut buffer = vec![0; 256 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
    }
    Ok((
        channels,
        digest.map(|digest| format!("{:x}", digest.finalize())),
    ))
}
