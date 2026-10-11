use super::*;

impl Accounts {
    pub fn users(&self) -> Result<Value> {
        Ok(json!(self
            .data()?
            .users
            .iter()
            .filter(|u| u.deleted_at.is_none())
            .map(public_user)
            .collect::<Vec<_>>()))
    }
    pub fn groups(&self) -> Result<Value> {
        Ok(serde_json::to_value(self.data()?.groups)?)
    }
    /// Keep the identity for history, remove credentials and all access grants.
    pub fn delete_user(&self, actor: &Principal, id: &str) -> Result<()> {
        security::id(id)?;
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        ensure!(
            self.current(actor)?.can("users.manage"),
            "Forbidden: users.manage required"
        );
        let mut data = self.data()?;
        let user = data
            .users
            .iter_mut()
            .find(|u| u.id == id && u.deleted_at.is_none())
            .context("User not found")?;
        user.enabled = false;
        user.deleted_at = Some(Utc::now().to_rfc3339());
        user.password_hash.clear();
        user.groups.clear();
        user.permissions.clear();
        user.auth_version += 1;
        let details = json!({"user_id":user.id,"name":user.name});
        ensure_admin(&data)?;
        self.save(&data)?;
        self.audit(&actor.id, "users.delete", &details)
    }
    pub fn update_profile(&self, actor: &Principal, request: &Value) -> Result<Principal> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        self.current(actor)?;
        let mut data = self.data()?;
        let user = data
            .users
            .iter_mut()
            .find(|u| u.id == actor.id && u.deleted_at.is_none())
            .context("User not found")?;
        user.name = text(request, "name")?;
        self.save(&data)?;
        self.audit(&actor.id, "users.profile", &json!({"name":request["name"]}))?;
        self.current(actor)
    }
    pub fn save_user(
        &self,
        actor: &Principal,
        user_id: Option<&str>,
        request: &Value,
    ) -> Result<Value> {
        ensure!(
            self.current(actor)?.can("users.manage"),
            "Forbidden: users.manage required"
        );
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        let mut data = self.data()?;
        let mut user = if let Some(id) = user_id {
            security::id(id)?;
            data.users
                .iter()
                .find(|u| u.id == id && u.deleted_at.is_none())
                .context("User not found")?
                .clone()
        } else {
            new_user(request)?
        };
        if user_id.is_some() {
            user.name = text(request, "name")?;
            user.enabled = request["enabled"].as_bool().context("Provide enabled")?;
            if let Some(password) = request["password"].as_str().filter(|p| !p.is_empty()) {
                user.password_hash = hash_password(password)?;
            }
            user.auth_version += 1;
        }
        user.groups = serde_json::from_value(request["groups"].clone())?;
        ensure!(
            user.groups
                .iter()
                .all(|id| data.groups.iter().any(|g| &g.id == id)),
            "Unknown group"
        );
        user.permissions = permissions(&request["permissions"])?;
        ensure!(
            !data
                .users
                .iter()
                .any(|u| u.deleted_at.is_none() && u.username == user.username && u.id != user.id),
            "Username already exists"
        );
        let result = public_user(&user);
        if let Some(saved) = data.users.iter_mut().find(|u| u.id == user.id) {
            *saved = user;
        } else {
            data.users.push(user);
        }
        ensure_admin(&data)?;
        self.save(&data)?;
        self.audit(&actor.id, "users.save", &json!({"user_id":result["id"]}))?;
        Ok(result)
    }
    pub fn save_group(
        &self,
        actor: &Principal,
        group_id: Option<&str>,
        request: &Value,
    ) -> Result<Value> {
        ensure!(
            self.current(actor)?.can("groups.manage"),
            "Forbidden: groups.manage required"
        );
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        let mut data = self.data()?;
        let group = Group {
            id: group_id
                .map(str::to_owned)
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            name: text(request, "name")?,
            permissions: permissions(&request["permissions"])?,
        };
        security::id(&group.id)?;
        ensure!(
            !data
                .groups
                .iter()
                .any(|g| g.name == group.name && g.id != group.id),
            "Group name already exists"
        );
        if let Some(saved) = data.groups.iter_mut().find(|g| g.id == group.id) {
            *saved = group.clone();
        } else {
            ensure!(group_id.is_none(), "Group not found");
            data.groups.push(group.clone());
        }
        ensure_admin(&data)?;
        self.save(&data)?;
        self.audit(&actor.id, "groups.save", &json!({"group_id":group.id}))?;
        Ok(serde_json::to_value(group)?)
    }
    pub fn delete_group(&self, actor: &Principal, id: &str) -> Result<()> {
        ensure!(
            self.current(actor)?.can("groups.manage"),
            "Forbidden: groups.manage required"
        );
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        let mut data = self.data()?;
        ensure!(data.groups.iter().any(|g| g.id == id), "Group not found");
        data.groups.retain(|g| g.id != id);
        for user in &mut data.users {
            user.groups.retain(|g| g != id);
        }
        ensure_admin(&data)?;
        self.save(&data)?;
        self.audit(&actor.id, "groups.delete", &json!({"group_id":id}))
    }
    pub fn save_settings(&self, actor: &Principal, request: &Value) -> Result<Value> {
        ensure!(
            self.current(actor)?.can("settings.manage"),
            "Forbidden: settings.manage required"
        );
        let settings: WorkflowSettings = serde_json::from_value(request.clone())?;
        ensure!(
            (1..=168).contains(&settings.session_hours),
            "Session duration must be 1–168 hours"
        );
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        let mut data = self.data()?;
        data.settings = settings;
        self.save(&data)?;
        self.audit(&actor.id, "settings.save", request)?;
        Ok(request.clone())
    }
    pub fn change_password(&self, actor: &Principal, request: &Value) -> Result<()> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        let mut data = self.data()?;
        let user = data
            .users
            .iter_mut()
            .find(|u| u.id == actor.id)
            .context("User not found")?;
        ensure!(
            verify_password(
                request["current_password"].as_str().unwrap_or(""),
                &user.password_hash
            ),
            "Unauthenticated: current password is incorrect"
        );
        user.password_hash =
            hash_password(request["password"].as_str().context("Provide password")?)?;
        user.auth_version += 1;
        self.save(&data)?;
        self.audit(&actor.id, "users.password", &json!({}))
    }
}
fn public_user(user: &User) -> Value {
    json!({"id":user.id,"username":user.username,"name":user.name,"enabled":user.enabled,"groups":user.groups,"permissions":user.permissions})
}
fn ensure_admin(data: &AccountData) -> Result<()> {
    ensure!(
        data.users.iter().any(|u| u.enabled
            && [
                "users.manage",
                "groups.manage",
                "settings.manage",
                "projects.admin"
            ]
            .iter()
            .all(|p| principal(data, u).can(p))),
        "Keep at least one enabled administrator with account and project management permissions"
    );
    Ok(())
}
