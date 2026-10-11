use super::*;

impl Accounts {
    pub fn set_project_member(
        &self,
        actor: &Principal,
        folder: &Path,
        user_id: &str,
        assigned: bool,
    ) -> Result<()> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        ensure!(
            self.current(actor)?.can("users.manage"),
            "Forbidden: users.manage required"
        );
        self.require_project(actor, folder, "project.manage")?;
        ensure!(
            self.data()?
                .users
                .iter()
                .any(|u| u.id == user_id && u.deleted_at.is_none()),
            "User not found"
        );
        let path = contained_path(folder, "access.json")?;
        let mut members: Vec<Member> = if path.is_file() {
            serde_json::from_value(read_json(&path)?["members"].clone())?
        } else {
            vec![]
        };
        let present = members.iter().any(|m| m.user_id == user_id);
        if present == assigned {
            return Ok(());
        }
        if assigned {
            members.push(Member {
                user_id: user_id.into(),
                groups: vec![],
                permissions: BTreeSet::from(["project.view".into()]),
                denied_permissions: BTreeSet::new(),
            });
        } else {
            members.retain(|m| m.user_id != user_id);
        }
        write_json(&path, &json!({"format_version":1,"members":members}))?;
        append_audit(
            &contained_path(folder, "audit/access.jsonl")?,
            &json!({"at":Utc::now().to_rfc3339(),"actor_id":actor.id,"actor_name":actor.name,"action":"member.assignment","user_id":user_id,"assigned":assigned}),
        )
    }
    pub fn project_permissions(
        &self,
        actor: &Principal,
        folder: &Path,
    ) -> Result<BTreeSet<String>> {
        let actor = self.current(actor)?;
        if actor.can("projects.admin") {
            return Ok(actor.permissions.clone());
        }
        let access = contained_path(folder, "access.json")?;
        if !access.is_file() {
            return Ok(BTreeSet::new());
        }
        let members: Vec<Member> = serde_json::from_value(read_json(&access)?["members"].clone())?;
        let Some(member) = members.iter().find(|m| m.user_id == actor.id) else {
            return Ok(BTreeSet::new());
        };
        let data = self.data()?;
        let mut effective = actor.permissions.clone();
        for group in data.groups.iter().filter(|g| member.groups.contains(&g.id)) {
            effective.extend(group.permissions.iter().cloned());
        }
        effective.extend(member.permissions.iter().cloned());
        effective.retain(|p| !member.denied_permissions.contains(p) && !system_permission(p));
        Ok(effective)
    }
    pub fn require_project(
        &self,
        actor: &Principal,
        folder: &Path,
        permission: &str,
    ) -> Result<()> {
        let permissions = self.project_permissions(actor, folder)?;
        ensure!(
            permissions.contains("project.view") && permissions.contains(permission),
            "Forbidden: project access and {permission} required"
        );
        Ok(())
    }
    pub fn members(&self, folder: &Path) -> Result<Value> {
        let path = contained_path(folder, "access.json")?;
        let members: Vec<Member> = if path.is_file() {
            serde_json::from_value(read_json(&path)?["members"].clone())?
        } else {
            vec![]
        };
        let data = self.data()?;
        Ok(json!(members
            .iter()
            .map(|m| {
                let user = data.users.iter().find(|u| u.id == m.user_id);
                let mut value = serde_json::to_value(m).unwrap();
                value["name"] = json!(user.map(|u| u.name.as_str()).unwrap_or("Unknown user"));
                value["enabled"] = json!(user.is_some_and(|u| u.enabled));
                value["effective_permissions"] = json!(user
                    .filter(|u| u.enabled)
                    .map(|u| self.project_permissions(&principal(&data, u), folder))
                    .transpose()?
                    .unwrap_or_default());
                Ok::<_, anyhow::Error>(value)
            })
            .collect::<Result<Vec<_>>>()?))
    }
    pub fn project_directory(&self, folder: &Path) -> Result<Value> {
        let data = self.data()?;
        let mut users = vec![];
        for user in data.users.iter().filter(|u| u.enabled) {
            let actor = principal(&data, user);
            let permissions = self.project_permissions(&actor, folder)?;
            if permissions.contains("project.view") {
                users.push(json!({"id":user.id,"username":user.username,"name":user.name,"permissions":permissions}));
            }
        }
        Ok(json!(users))
    }
    pub fn save_members(&self, actor: &Principal, folder: &Path, request: &Value) -> Result<Value> {
        self.require_project(actor, folder, "project.manage")?;
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Accounts unavailable"))?;
        let _lock = self.lock()?;
        let members: Vec<Member> = serde_json::from_value(request["members"].clone())?;
        let data = self.data()?;
        let mut seen = BTreeSet::new();
        for member in &members {
            ensure!(seen.insert(&member.user_id), "Duplicate member");
            ensure!(
                data.users.iter().any(|u| u.id == member.user_id),
                "User not found"
            );
            ensure!(
                member
                    .groups
                    .iter()
                    .all(|g| data.groups.iter().any(|group| &group.id == g)),
                "Group not found"
            );
            for p in member.permissions.iter().chain(&member.denied_permissions) {
                ensure!(
                    PERMISSIONS.contains(&p.as_str()) && !system_permission(p),
                    "Project membership cannot grant system permissions"
                );
            }
        }
        let path = contained_path(folder, "access.json")?;
        write_json(&path, &json!({"format_version":1,"members":members}))?;
        append_audit(
            &contained_path(folder, "audit/access.jsonl")?,
            &json!({"at":Utc::now().to_rfc3339(),"actor_id":actor.id,"actor_name":actor.name,"action":"members.save","members":members}),
        )?;
        self.members(folder)
    }
}
pub fn system_permission(permission: &str) -> bool {
    matches!(
        permission,
        "projects.admin" | "users.manage" | "groups.manage" | "settings.manage"
    )
}
