//! Dataset membership uses valid task and section approvals.
use super::*;

impl Store {
    pub(in crate::store) fn latest_approved_recording(
        &self,
        project: &str,
        recording: &str,
        channel: Option<&str>,
    ) -> Result<(Recording, Value)> {
        let source = self
            .approved_sources(project)?
            .into_iter()
            .find(|s| {
                s.task.recording_id == recording
                    && s.task.dataset_id.is_none()
                    && channel.is_none_or(|channel| {
                        s.revision
                            .scopes
                            .iter()
                            .any(|scope| scope.channel_id == channel)
                    })
            })
            .context("Approve a revision before exporting annotation data")?;
        Ok((
            self.revision_recording(project, &source.task, &source.revision)?,
            source.reference(),
        ))
    }
    pub(in crate::store) fn approved_csv_data(
        &self,
        project: &str,
        dataset: &str,
    ) -> Result<Vec<u8>> {
        let source = self
            .approved_sources(project)?
            .into_iter()
            .find(|s| s.snapshot["datasets"].get(dataset).is_some())
            .context("Approve a revision of this CSV label table before downloading")?;
        let data = snapshot_dataset(&source.folder, &source.snapshot, dataset)?;
        let columns = data["columns"]
            .as_array()
            .context("CSV snapshot needs columns")?;
        let mut csv = columns
            .iter()
            .map(|c| super::super::csv_cell(&c["name"]))
            .collect::<Vec<_>>()
            .join(",");
        csv.push_str("\r\n");
        let mut rows: Vec<_> = data["entries"]
            .as_object()
            .context("CSV snapshot needs rows")?
            .values()
            .collect();
        rows.sort_by_key(|r| r["frame"].as_u64());
        for row in rows {
            csv.push_str(
                &columns
                    .iter()
                    .map(|c| {
                        super::super::csv_cell(&row["values"][c["name"].as_str().unwrap_or("")])
                    })
                    .collect::<Vec<_>>()
                    .join(","),
            );
            csv.push_str("\r\n");
        }
        Ok(csv.into_bytes())
    }
    pub(in crate::store) fn eligible_rows<'a>(
        &self,
        recording: &'a Recording,
        channel: &str,
    ) -> Result<Vec<(usize, &'a Value)>> {
        if self.accounts.setup_required()? {
            return super::super::reviewed_rows(recording, channel);
        }
        let approvals = self.approved_scopes(recording)?;
        let mut approved_rows = std::collections::HashMap::new();
        for source in self
            .approved_sources(field(&recording.project, "uuid")?)?
            .into_iter()
            .filter(|s| {
                s.task.recording_id == recording.item["uuid"].as_str().unwrap_or("")
                    && s.task.dataset_id.is_none()
            })
        {
            let frozen = snapshot_annotations(&recording.folder, &source.snapshot)?;
            if frozen["classes"] != recording.annotations["classes"]
                || frozen["class_uuids"] != recording.annotations["class_uuids"]
            {
                continue;
            }
            for (number, row) in frozen["annotations"][channel]
                .as_object()
                .into_iter()
                .flatten()
            {
                approved_rows
                    .entry(number.clone())
                    .or_insert_with(|| row.clone());
            }
        }
        let require_approval = self.accounts.settings()?.require_approval_for_datasets;
        let mut rows = vec![];
        for (number, row) in recording.annotations["annotations"][channel]
            .as_object()
            .into_iter()
            .flatten()
        {
            let number = number.parse()?;
            if included(row)
                && (!require_approval
                    || (approvals.iter().any(|s| s.contains(channel, number))
                        && approved_rows
                            .get(&number.to_string())
                            .is_some_and(|saved| same_json(saved, row))))
            {
                rows.push((number, row));
            }
        }
        rows.sort_by_key(|(n, _)| *n);
        Ok(rows)
    }
    pub(in crate::store) fn approved_dataset_scopes(
        &self,
        recording: &Recording,
        dataset_id: &str,
    ) -> Result<Vec<Scope>> {
        let project = field(&recording.project, "uuid")?;
        let mut scopes = vec![];
        for source in self.approved_sources(project)?.into_iter().filter(|s| {
            s.task.dataset_id.as_deref() == Some(dataset_id)
                && s.task.recording_id == recording.item["uuid"].as_str().unwrap_or("")
        }) {
            scopes.extend(source.revision.scopes);
        }
        Ok(scopes)
    }
    pub(in crate::store) fn approved_scopes(&self, recording: &Recording) -> Result<Vec<Scope>> {
        let project = field(&recording.project, "uuid")?;
        let mut scopes = vec![];
        for source in self.approved_sources(project)?.into_iter().filter(|s| {
            s.task.dataset_id.is_none()
                && s.task.recording_id == recording.item["uuid"].as_str().unwrap_or("")
        }) {
            scopes.extend(source.revision.scopes);
        }
        Ok(scopes)
    }
}
