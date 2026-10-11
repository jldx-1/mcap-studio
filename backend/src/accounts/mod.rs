//! Accounts and groups are workspace metadata; membership belongs to each project.
//! Sessions contain only a hashed, random token and never a password.
mod management;
mod membership;
mod model;
mod security;
use crate::storage::{contained_path, read_json, write_json};
use anyhow::{ensure, Context, Result};
use chrono::Utc;
use fs2::FileExt;
pub use model::{AccountData, Group, Member, Principal, WorkflowSettings, PERMISSIONS};
use model::{Session, User};
use security::{hash_password, session_key, verify_password};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use uuid::Uuid;

pub struct Accounts {
    root: PathBuf,
    gate: Mutex<()>,
    attempts: Mutex<HashMap<String, (i64, u32)>>,
}

impl Accounts {
    pub fn new(workspace: &Path) -> Result<Self> {
        let root = workspace.join("data/accounts");
        fs::create_dir_all(root.join("sessions"))?;
        Ok(Self {
            root,
            gate: Mutex::new(()),
            attempts: Mutex::new(HashMap::new()),
        })
    }

    fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(".accounts.lock"))?;
        file.lock_exclusive()?;
        Ok(file)
    }
    fn data(&self) -> Result<AccountData> {
        let path = self.root.join("accounts.json");
        if path.exists() {
            Ok(serde_json::from_value(read_json(&path)?)?)
        } else {
            Ok(AccountData::default())
        }
    }
    fn save(&self, data: &AccountData) -> Result<()> {
        write_json(
            &self.root.join("accounts.json"),
            &serde_json::to_value(data)?,
        )
    }
    pub fn setup_required(&self) -> Result<bool> {
        Ok(self.data()?.users.is_empty())
    }
    pub fn settings(&self) -> Result<WorkflowSettings> {
        Ok(self.data()?.settings)
    }

    pub fn setup(&self, request: &Value) -> Result<(Principal, String)> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        let mut data = self.data()?;
        ensure!(data.users.is_empty(), "Account setup already exists");
        let defaults = [
            ("Administrators", PERMISSIONS.to_vec()),
            (
                "Annotators",
                vec![
                    "project.view",
                    "mcap.view",
                    "annotation.view",
                    "annotation.create",
                    "annotation.edit",
                    "annotation.delete",
                    "annotation.submit",
                ],
            ),
            (
                "Reviewers",
                vec![
                    "project.view",
                    "mcap.view",
                    "annotation.view",
                    "review.view",
                    "review.approve",
                    "review.reject",
                    "review.comment",
                ],
            ),
            (
                "Project Managers",
                vec![
                    "project.view",
                    "project.manage",
                    "mcap.view",
                    "annotation.view",
                    "review.view",
                    "review.assign",
                    "labels.manage",
                ],
            ),
            (
                "Dataset Managers",
                vec![
                    "project.view",
                    "mcap.view",
                    "annotation.view",
                    "datasets.generate",
                    "datasets.export",
                ],
            ),
        ];
        for (name, permissions) in defaults {
            data.groups.push(Group {
                id: Uuid::new_v4().to_string(),
                name: name.into(),
                permissions: permissions.into_iter().map(str::to_owned).collect(),
            });
        }
        let mut user = new_user(request)?;
        user.groups = vec![data.groups[0].id.clone()];
        data.users.push(user.clone());
        self.save(&data)?;
        self.audit(&user.id, "accounts.setup", &json!({}))?;
        self.issue_session(&data, &user)
    }

    pub fn login(&self, request: &Value) -> Result<(Principal, String)> {
        let username = request["username"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let now = Utc::now().timestamp();
        attempts.retain(|_, (since, _)| now - *since < 900);
        ensure!(
            attempts.get(&username).is_none_or(|(_, count)| *count < 10),
            "Too many login attempts; try again in 15 minutes"
        );
        let data = self.data()?;
        let user = data
            .users
            .iter()
            .find(|u| u.username == username && u.enabled);
        let password = request["password"].as_str().unwrap_or("");
        let valid = user.is_some_and(|u| verify_password(password, &u.password_hash));
        if !valid {
            ensure!(
                attempts.len() < 512 || attempts.contains_key(&username),
                "Too many login attempts; try again in 15 minutes"
            );
            attempts.entry(username).or_insert((now, 0)).1 += 1;
            anyhow::bail!("Unauthenticated: invalid username or password");
        }
        attempts.remove(&username);
        self.issue_session(&data, user.unwrap())
    }

    fn issue_session(&self, data: &AccountData, user: &User) -> Result<(Principal, String)> {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let session = Session {
            user_id: user.id.clone(),
            auth_version: user.auth_version,
            expires_at: Utc::now().timestamp() + i64::from(data.settings.session_hours) * 3600,
        };
        write_json(
            &self
                .root
                .join("sessions")
                .join(format!("{}.json", session_key(&token)?)),
            &serde_json::to_value(session)?,
        )?;
        Ok((principal(data, user), token))
    }
    pub fn authenticate(&self, token: &str) -> Result<Principal> {
        let path = self
            .root
            .join("sessions")
            .join(format!("{}.json", session_key(token)?));
        let session: Session =
            serde_json::from_value(read_json(&path).context("Unauthenticated")?)?;
        ensure!(
            session.expires_at > Utc::now().timestamp(),
            "Unauthenticated: session expired"
        );
        let data = self.data()?;
        let user = data
            .users
            .iter()
            .find(|u| {
                u.id == session.user_id && u.enabled && u.auth_version == session.auth_version
            })
            .context("Unauthenticated: session revoked")?;
        Ok(principal(&data, user))
    }
    pub fn logout(&self, token: &str) -> Result<()> {
        let path = self
            .root
            .join("sessions")
            .join(format!("{}.json", session_key(token)?));
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
    fn current(&self, actor: &Principal) -> Result<Principal> {
        let data = self.data()?;
        let user = data
            .users
            .iter()
            .find(|u| u.id == actor.id && u.enabled && u.auth_version == actor.auth_version)
            .context("Unauthenticated: account access changed")?;
        Ok(principal(&data, user))
    }
    fn audit(&self, actor: &str, action: &str, details: &Value) -> Result<()> {
        append_audit(
            &self.root.join("audit.jsonl"),
            &json!({"at":Utc::now().to_rfc3339(),"actor_id":actor,"action":action,"details":details}),
        )
    }
}

pub(crate) fn append_audit(path: &Path, event: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;
    file.lock_exclusive()?;
    let mut row = serde_json::to_vec(event)?;
    row.push(b'\n');
    file.write_all(&row)?;
    file.sync_data()?;
    Ok(())
}
fn principal(data: &AccountData, user: &User) -> Principal {
    let mut permissions = user.permissions.clone();
    for group in &data.groups {
        if user.groups.contains(&group.id) {
            permissions.extend(group.permissions.iter().cloned());
        }
    }
    Principal {
        id: user.id.clone(),
        name: user.name.clone(),
        username: user.username.clone(),
        permissions,
        group_names: data
            .groups
            .iter()
            .filter(|g| user.groups.contains(&g.id))
            .map(|g| g.name.clone())
            .collect(),
        auth_version: user.auth_version,
    }
}
fn text(request: &Value, key: &str) -> Result<String> {
    let value = request[key]
        .as_str()
        .context(format!("Provide {key}"))?
        .trim();
    ensure!(
        !value.is_empty() && value.len() <= 120 && !value.chars().any(char::is_control),
        "Invalid {key}"
    );
    Ok(value.into())
}
fn permissions(value: &Value) -> Result<BTreeSet<String>> {
    let permissions: BTreeSet<String> = serde_json::from_value(value.clone())?;
    ensure!(
        permissions
            .iter()
            .all(|p| PERMISSIONS.contains(&p.as_str())),
        "Unknown permission"
    );
    Ok(permissions)
}
fn new_user(request: &Value) -> Result<User> {
    let username = text(request, "username")?.to_lowercase();
    ensure!(
        username
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b)),
        "Invalid username"
    );
    Ok(User {
        id: Uuid::new_v4().to_string(),
        username,
        name: text(request, "name")?,
        enabled: true,
        deleted_at: None,
        groups: vec![],
        permissions: BTreeSet::new(),
        auth_version: 0,
        password_hash: hash_password(request["password"].as_str().context("Provide password")?)?,
    })
}
