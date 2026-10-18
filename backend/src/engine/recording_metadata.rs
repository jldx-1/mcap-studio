//! Recording metrics without decoding messages or building frame previews.
use anyhow::Result;
use mcap::sans_io::{SummaryReadEvent, SummaryReader, SummaryReaderOptions};
use serde_json::{json, Value};
use std::collections::HashMap;
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
            let topics: Vec<_> = summary
                .channels
                .values()
                .map(|channel| {
                    topic_details(
                        channel,
                        stats
                            .channel_message_counts
                            .get(&channel.id)
                            .copied()
                            .unwrap_or(0),
                    )
                })
                .collect();
            return Ok(
                json!({"file_size_bytes":size,"topic_count":stats.channel_count,
                "duration_seconds":stats.message_end_time.saturating_sub(stats.message_start_time) as f64 / 1e9,"topic_details":topics}),
            );
        }
    }
    let mut start = u64::MAX;
    let mut end = 0;
    let mut counts = HashMap::<u16, u64>::new();
    let channels = super::stream_messages(path, |message| {
        start = start.min(message.log_time);
        end = end.max(message.log_time);
        *counts.entry(message.channel.id).or_default() += 1;
        Ok(())
    })?;
    let topics: Vec<_> = channels
        .values()
        .map(|channel| topic_details(channel, counts.get(&channel.id).copied().unwrap_or(0)))
        .collect();
    Ok(json!({"file_size_bytes":size,"topic_count":channels.len(),
        "duration_seconds":end.saturating_sub(start) as f64 / 1e9,"topic_details":topics}))
}

fn topic_details(channel: &mcap::Channel<'_>, count: u64) -> Value {
    let schema = channel
        .schema
        .as_ref()
        .map(|schema| schema.name.as_str())
        .unwrap_or("");
    let identity =
        format!("{} {} {}", channel.topic, schema, channel.message_encoding).to_ascii_lowercase();
    let modality = if [
        "metadata",
        "camerainfo",
        "cameracalibration",
        "/camera/info",
    ]
    .iter()
    .any(|name| identity.contains(name))
    {
        "Metadata"
    } else if ["image", "video", "h264", "jpeg", "mjpeg", "/camera/rgb"]
        .iter()
        .any(|name| identity.contains(name))
    {
        "Video"
    } else if identity.contains("imu") {
        "IMU"
    } else if ["pointcloud", "point_cloud", "lidar", "laser"]
        .iter()
        .any(|name| identity.contains(name))
    {
        "Point cloud"
    } else if ["navsat", "gps", "gnss"]
        .iter()
        .any(|name| identity.contains(name))
    {
        "GPS"
    } else if [
        "joint",
        "motor",
        "pose",
        "twist",
        "transform",
        "/tf",
        "motion",
    ]
    .iter()
    .any(|name| identity.contains(name))
    {
        "Motion"
    } else {
        "Structured"
    };
    json!({"channel_id":channel.id.to_string(),"topic":channel.topic,"encoding":channel.message_encoding,"schema_name":schema,"message_count":count,"modality":modality})
}
