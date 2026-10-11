//! Project-contained task files and audit events.
use super::*;

pub(super) fn task_path(recording: &str, task: &str) -> Result<String> {
    canonical_uuid(recording)?;
    canonical_uuid(task)?;
    Ok(format!("reviews/{recording}/{task}.json"))
}
pub(super) fn read_tasks(folder: &Path, recording: Option<&str>) -> Result<Vec<Task>> {
    let root = contained_path(folder, "reviews")?;
    if !root.is_dir() {
        return Ok(vec![]);
    }
    let folders = if let Some(recording) = recording {
        canonical_uuid(recording)?;
        vec![contained_path(folder, &format!("reviews/{recording}"))?]
    } else {
        fs::read_dir(&root)?
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    };
    let mut tasks: Vec<Task> = vec![];
    for dir in folders.into_iter().filter(|p| p.is_dir()) {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| canonical_uuid(s).is_ok())
            {
                let relative = path.strip_prefix(folder)?;
                tasks.push(serde_json::from_value(read_json(&contained_path(
                    folder,
                    &relative.to_string_lossy(),
                )?)?)?);
            }
        }
    }
    tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(tasks)
}
pub(super) fn save_task(folder: &Path, task: &Task) -> Result<()> {
    write_json(
        &contained_path(folder, &task_path(&task.recording_id, &task.id)?)?,
        &serde_json::to_value(task)?,
    )
}
pub(super) fn event(task: &mut Task, actor: &Principal, action: &str, details: Value) {
    task.version += 1;
    task.updated_at = now();
    task.history.push(json!({"at":task.updated_at,"actor_id":actor.id,"actor_name":actor.name,"action":action,"details":details}));
}
pub(super) fn audit(folder: &Path, task: &Task) -> Result<()> {
    append_audit(
        &contained_path(folder, "audit/reviews.jsonl")?,
        &json!({"task_id":task.id,"recording_id":task.recording_id,"event":task.history.last()}),
    )
}
