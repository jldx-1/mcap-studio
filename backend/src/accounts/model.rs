use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PERMISSIONS: &[&str] = &[
    "projects.admin",
    "project.view",
    "project.manage",
    "mcap.view",
    "annotation.view",
    "annotation.create",
    "annotation.edit",
    "annotation.delete",
    "annotation.submit",
    "review.view",
    "review.approve",
    "review.reject",
    "review.comment",
    "review.assign",
    "labels.manage",
    "datasets.generate",
    "datasets.export",
    "users.manage",
    "groups.manage",
    "settings.manage",
];

#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub deleted_at: Option<String>,
    pub groups: Vec<String>,
    pub permissions: BTreeSet<String>,
    #[serde(default)]
    pub auth_version: u64,
    pub password_hash: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub permissions: BTreeSet<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkflowSettings {
    pub independent_reviewer: bool,
    pub require_approval_for_datasets: bool,
    pub session_hours: u32,
}
impl Default for WorkflowSettings {
    fn default() -> Self {
        Self {
            independent_reviewer: true,
            require_approval_for_datasets: true,
            session_hours: 12,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AccountData {
    pub users: Vec<User>,
    pub groups: Vec<Group>,
    #[serde(default)]
    pub settings: WorkflowSettings,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Member {
    pub user_id: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub permissions: BTreeSet<String>,
    #[serde(default)]
    pub denied_permissions: BTreeSet<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub username: String,
    pub name: String,
    pub permissions: BTreeSet<String>,
    pub group_names: Vec<String>,
    #[serde(skip_serializing)]
    pub auth_version: u64,
}
impl Principal {
    pub fn can(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct Session {
    pub user_id: String,
    pub auth_version: u64,
    pub expires_at: i64,
}
