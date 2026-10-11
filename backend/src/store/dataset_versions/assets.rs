//! Version-scoped previews expose only generated, pinned training inputs.
use super::*;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
};

impl Store {
    pub fn dataset_version_artifacts(
        &self,
        project: &str,
        dataset: &str,
        number: u64,
    ) -> Result<Value> {
        let folder = self.folder(project)?;
        let version = self.dataset_version(project, dataset, number)?;
        let hash = digest(&version)?;
        let formats:Vec<_>=["csv","yolo"].into_iter().filter(|format|read_json(&contained_path(&folder,&format!("datasets/{dataset}/versions/{number:012}/generated/{format}/manifest.json")).unwrap_or_default()).is_ok_and(|m|m["source_version_sha256"]==hash)).collect();
        Ok(json!({"formats":formats}))
    }
    pub fn dataset_version_input(
        &self,
        project: &str,
        dataset: &str,
        number: u64,
        key: &str,
        input_name: &str,
    ) -> Result<(Vec<u8>, String)> {
        let sample_number = key.parse::<u64>()?;
        ensure!(format!("{sample_number:016}") == key, "Invalid sample key");
        let folder = self.folder(project)?;
        let version = self.dataset_version(project, dataset, number)?;
        let base = format!("datasets/{dataset}/versions/{number:012}/generated");
        let mut selected = None;
        for format in [
            version["default_format"].as_str().unwrap_or("csv"),
            "csv",
            "yolo",
        ] {
            let path = contained_path(&folder, &format!("{base}/{format}/manifest.json"))?;
            if path.is_file() {
                selected = Some((format.to_owned(), read_json(&path)?));
                break;
            }
        }
        let (format, manifest) =
            selected.context("Build this dataset version before previewing its inputs")?;
        ensure!(
            manifest["source_version_sha256"] == digest(&version)?,
            "Generated inputs do not match dataset version"
        );
        let bucket = format!("{:012}", sample_number / PAGE_SIZE);
        let page = manifest["sample_pages"]
            .get(&bucket)
            .context("Dataset sample not found")?;
        let rows = read_pages(
            &folder,
            &format!("{base}/{format}/samples"),
            &json!({bucket:page}),
        )?;
        let sample = rows.get(key).context("Dataset sample not found")?;
        let input = sample["inputs"]
            .get(input_name)
            .context("Dataset input not found")?;
        if input["kind"] == "image" {
            let path = contained_path(&folder, field(input, "image")?)?;
            let allowed = contained_path(&folder, &format!("{base}/{format}/images"))?;
            ensure!(
                path.starts_with(allowed),
                "Dataset image must belong to this version"
            );
            Ok((fs::read(path)?, "image/png".into()))
        } else {
            let path = contained_path(&folder, field(input, "records")?)?;
            let allowed = contained_path(&folder, &format!("{base}/{format}/sensors"))?;
            ensure!(
                path.starts_with(allowed),
                "Dataset sensor data must belong to this version"
            );
            let mut file = BufReader::new(File::open(path)?);
            file.seek(SeekFrom::Start(
                input["offset"].as_u64().context("Missing sensor offset")?,
            ))?;
            let mut line = String::new();
            file.take(32 * 1024 * 1024).read_line(&mut line)?;
            Ok((line.into_bytes(), "application/json".into()))
        }
    }
}
