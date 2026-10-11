//! Administrative project assignment keeps unrelated membership grants intact.
use super::*;
use crate::accounts::Principal;
use serde_json::json;

impl Store {
    pub fn user_project_assignments(&self, actor: &Principal, user_id: &str) -> Result<Value> {
        canonical_uuid(user_id)?;
        ensure!(
            actor.can("users.manage"),
            "Forbidden: users.manage required"
        );
        ensure!(
            self.accounts
                .users()?
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["id"] == user_id),
            "User not found"
        );
        let mut projects = vec![];
        for project in self.list_projects()?.as_array().into_iter().flatten() {
            let id = field(project, "uuid")?;
            let folder = self.folder(id)?;
            let permissions = self.accounts.project_permissions(actor, &folder)?;
            if permissions.contains("project.view") && permissions.contains("project.manage") {
                let assigned = self
                    .accounts
                    .members(&folder)?
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["user_id"] == user_id);
                projects.push(json!({"uuid":id,"name":project["name"],"assigned":assigned}));
            }
        }
        Ok(json!(projects))
    }
    pub fn save_user_projects(
        &self,
        actor: &Principal,
        user_id: &str,
        request: &Value,
    ) -> Result<Value> {
        let options = self.user_project_assignments(actor, user_id)?;
        let wanted: std::collections::BTreeSet<String> =
            serde_json::from_value(request["project_ids"].clone())?;
        ensure!(
            wanted
                .iter()
                .all(|id| options.as_array().unwrap().iter().any(|p| p["uuid"] == *id)),
            "Forbidden: only projects you manage can be assigned"
        );
        for project in options.as_array().unwrap() {
            let id = field(project, "uuid")?;
            self.accounts.set_project_member(
                actor,
                &self.folder(id)?,
                user_id,
                wanted.contains(id),
            )?;
        }
        self.user_project_assignments(actor, user_id)
    }
}
