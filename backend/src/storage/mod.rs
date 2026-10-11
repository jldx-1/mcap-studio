//! Portable folder persistence, independent of application state and routing.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, path::Path};

pub const PAGE_SIZE: u64 = 256;
pub const REGISTRIES: [&str; 4] = ["mcaps", "labels", "label_lists", "datasets"];

mod annotations;
mod datasets;
pub(crate) mod filesystem;
mod pages;
mod project_format;
pub(crate) use annotations::{annotation_buckets, read_annotation_subset, save_annotation_subset};
pub use annotations::{annotation_path, read_annotations, save_annotations};
use datasets::dataset_metadata;
pub use datasets::hydrate_dataset;
pub use filesystem::{contained_path, digest, file_hash, fingerprint, write_changed, write_json};
pub(crate) use filesystem::{copy_tree, list_files, read_json};
pub use pages::{read_pages, write_pages};
pub(crate) use project_format::{known_manifest_stamp, metadata_path, save_manifest_delta};
pub use project_format::{manifest_stamp, resolve_manifest, save_manifest};
