//! Task metadata is independent of annotation pages and raw MCAPs. All transitions
//! and annotation locks share the project filesystem lock.
mod annotations;
mod catalog;
mod creation;
mod eligibility;
mod migration;
mod model;
mod persistence;
mod queries;
mod revisions;
mod snapshot_storage;
mod snapshots;
mod workflow;
use super::{canonical_uuid, contained_path, field, now, read_json, same_json, Recording, Store};
use crate::{
    accounts::{append_audit, Principal},
    storage::{digest, write_json},
};
use anyhow::{ensure, Context, Result};
use model::{ReviewComment, Revision, Scope, SnapshotRef, Status, Task};
use serde_json::{json, Value};
use std::{fs, path::Path};
use uuid::Uuid;

pub(in crate::store) use catalog::{modality, ApprovedSource};
use creation::assign_reviewer;
use persistence::{audit, event, read_tasks, save_task};
use queries::previous_revision_summary;
use revisions::{read_revision, revision_list, save_revision};
use snapshot_storage::{publish_snapshot, read_snapshot, snapshot_annotations, snapshot_dataset};
pub(super) use snapshots::included;
use snapshots::{annotation_digest, count_annotations, validate_scope};
