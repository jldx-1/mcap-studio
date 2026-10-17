//! Recording metrics without decoding messages or building frame previews.
use anyhow::Result;
use mcap::sans_io::{SummaryReadEvent, SummaryReader, SummaryReaderOptions};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{BufReader, Read, Seek},
    path::Path,
};

pub fn recording_metadata(path: &Path) -> Result<Value> {
    let mut file = BufReader::new(File::open(path)?);
    let size = file.get_ref().metadata()?.len();
    let mut reader =
        SummaryReader::new_with_options(SummaryReaderOptions::default().with_file_size(size));
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
    if let Some(summary) = reader.finish() {
        if let Some(stats) = summary.stats {
            return Ok(
                json!({"file_size_bytes":size,"topic_count":stats.channel_count,
                "duration_seconds":stats.message_end_time.saturating_sub(stats.message_start_time) as f64 / 1e9}),
            );
        }
    }
    // Files without summary statistics still have message timestamps. Scan once,
    // retaining only channels and time bounds, and cache the result in the store.
    let mut start = u64::MAX;
    let mut end = 0;
    let channels = super::stream_messages(path, |message| {
        start = start.min(message.log_time);
        end = end.max(message.log_time);
        Ok(())
    })?;
    Ok(json!({"file_size_bytes":size,"topic_count":channels.len(),
        "duration_seconds":end.saturating_sub(start) as f64 / 1e9}))
}
