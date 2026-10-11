use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scope {
    pub channel_id: String,
    pub start_frame: usize,
    pub end_frame: usize,
}
impl Scope {
    pub fn contains(&self, channel: &str, number: usize) -> bool {
        self.channel_id == channel && (self.start_frame..=self.end_frame).contains(&number)
    }
    pub fn overlaps(&self, other: &Self) -> bool {
        self.channel_id == other.channel_id
            && self.start_frame <= other.end_frame
            && other.start_frame <= self.end_frame
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unlabeled,
    InProgress,
    Submitted,
    UnderReview,
    ChangesRequested,
    Approved,
    Superseded,
    Withdrawn,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub recording_id: String,
    #[serde(default)]
    pub dataset_id: Option<String>,
    pub title: String,
    pub scopes: Vec<Scope>,
    pub annotator_id: String,
    pub annotator_name: String,
    pub reviewer_id: Option<String>,
    pub reviewer_name: Option<String>,
    pub status: Status,
    #[serde(default)]
    pub current_revision: u64,
    #[serde(default)]
    pub working_dirty: bool,
    pub source_sha256: String,
    pub submitted_digest: Option<String>,
    pub frame_count: usize,
    pub annotated_count: usize,
    pub approved_sections: Vec<Scope>,
    pub history: Vec<Value>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnapshotRef {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ReviewComment {
    pub id: String,
    pub actor_id: String,
    pub actor_name: String,
    pub at: String,
    pub message: String,
    pub scope: Option<Scope>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Revision {
    pub id: String,
    pub task_id: String,
    pub recording_id: String,
    pub number: u64,
    pub scopes: Vec<Scope>,
    pub submitted_by_id: String,
    pub submitted_by_name: String,
    pub submitted_at: String,
    pub reviewed_at: Option<String>,
    #[serde(default)]
    pub review_started_at: Option<String>,
    /// Set only when the submitter explicitly selects themselves as reviewer.
    #[serde(default)]
    pub self_review: bool,
    pub reviewer_id: Option<String>,
    pub reviewer_name: Option<String>,
    pub status: Status,
    pub snapshot: SnapshotRef,
    #[serde(default = "snapshot_available")]
    pub snapshot_available: bool,
    pub frame_count: usize,
    pub annotated_count: usize,
    pub comments: Vec<ReviewComment>,
    pub version: u64,
}
fn snapshot_available() -> bool {
    true
}
