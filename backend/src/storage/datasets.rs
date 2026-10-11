//! Source dataset configuration and membership page persistence.
use super::*;

pub fn hydrate_dataset(folder: &Path, dataset: &mut Value) -> Result<()> {
    if dataset.get("entry_pages").is_some() {
        let id = dataset["uuid"].as_str().context("Missing dataset UUID")?;
        dataset["entries"] = json!(read_pages(
            folder,
            &format!("datasets/{id}/entries"),
            &dataset["entry_pages"]
        )?);
    }
    Ok(())
}

pub(super) fn dataset_metadata(folder: &Path, dataset: &Value) -> Result<Value> {
    let mut metadata = dataset.clone();
    if let Some(entries) = metadata.as_object_mut().unwrap().remove("entries") {
        let id = dataset["uuid"].as_str().context("Missing dataset UUID")?;
        let mut pages: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
        let mut sources: BTreeMap<(String, String), Value> = BTreeMap::new();
        for (key, entry) in entries.as_object().context("Invalid entries")? {
            let recording = entry["mcap_uuid"]
                .as_str()
                .context("Missing recording UUID")?;
            let channel = entry["channel_id"].as_str().context("Missing channel ID")?;
            let number = entry["frame"].as_u64().context("Missing frame number")?;
            ensure!(
                uuid::Uuid::parse_str(recording)?.to_string() == recording
                    && channel.parse::<u16>()?.to_string() == channel,
                "Invalid dataset identity"
            );
            ensure!(
                key == &format!("{recording}:{channel}:{number}"),
                "Dataset key does not match frame"
            );
            pages
                .entry(format!("{recording}/{channel}/{:012}", number / PAGE_SIZE))
                .or_default()
                .insert(key.clone(), entry.clone());
            let source = sources.entry((recording.to_owned(), channel.to_owned())).or_insert_with(|| json!({"mcap_uuid":recording,"channel_id":channel,"topic":entry["topic"],"modality":if dataset["format"] == "csv" {"structured"} else {"image"},"entry_count":0}));
            source["entry_count"] = json!(source["entry_count"].as_u64().unwrap() + 1);
        }
        metadata["entry_pages"] = write_pages(folder, &format!("datasets/{id}/entries"), pages)?;
        metadata["entry_count"] = json!(entries.as_object().unwrap().len());
        metadata["sources"] = json!(sources.into_values().collect::<Vec<_>>());
    }
    Ok(metadata)
}
