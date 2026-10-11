//! Bounded, immutable JSON pages shared by annotations and datasets.
use super::*;

/// Publish immutable bounded pages first, then atomically switch their metadata.
pub fn write_pages(
    folder: &Path,
    base: &str,
    groups: BTreeMap<String, Map<String, Value>>,
) -> Result<Value> {
    let mut pages = Map::new();
    for (group, rows) in groups {
        let value = json!(rows);
        let hash = digest(&value)?;
        let relative = format!("{base}/{group}-{hash}.json");
        let path = contained_path(folder, &relative)?;
        write_changed(&path, &value)?;
        pages.insert(
            group,
            json!({"path":relative,"sha256":hash,"count":rows.len()}),
        );
    }
    Ok(json!(pages))
}

pub fn read_pages(folder: &Path, base: &str, pages: &Value) -> Result<Map<String, Value>> {
    let mut rows = Map::new();
    for (group, page) in pages.as_object().context("Missing pages")? {
        let hash = page["sha256"].as_str().context("Missing page hash")?;
        let expected = format!("{base}/{group}-{hash}.json");
        ensure!(
            page["path"] == expected,
            "Page path does not match its identity"
        );
        let value = read_json(&contained_path(folder, &expected)?)?;
        ensure!(
            digest(&value)? == hash,
            "Source page hash does not match contents"
        );
        ensure!(
            value.as_object().context("Invalid page")?.len() as u64
                == page["count"].as_u64().context("Missing page count")?,
            "Page count does not match contents"
        );
        for (key, row) in value.as_object().context("Invalid page")? {
            ensure!(
                rows.insert(key.clone(), row.clone()).is_none(),
                "Duplicate source row"
            );
        }
    }
    Ok(rows)
}
