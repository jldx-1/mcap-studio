import { useCallback, useEffect, useState } from "react";
import { GroupManagement } from "../components/accounts/GroupManagement";
import { UserManagement } from "../components/accounts/UserManagement";
import { api, put } from "../services/http";
import { useAuth } from "../state/AuthContext";
import type { Group, ManagedUser, WorkflowSettings } from "../types/accounts";
import type { Perform } from "../types/workspace";

export function AdministrationPage({ busy, perform }: { busy: boolean; perform: Perform }) {
  const auth = useAuth();
  const permissions = auth.user?.permissions || [];
  const manageUsers = permissions.includes("users.manage"), manageGroups = permissions.includes("groups.manage"), manageSettings = permissions.includes("settings.manage");
  const [users,setUsers] = useState<ManagedUser[]>([]);const [groups,setGroups] = useState<Group[]>([]);
  const [settings,setSettings] = useState<WorkflowSettings | null>(auth.session?.workflow || null);
  const reload = useCallback(async () => {
    const values = await Promise.all([manageUsers ? api<ManagedUser[]>("/accounts/users") : [], (manageGroups || manageUsers) ? api<Group[]>("/accounts/groups") : [], manageSettings ? api<WorkflowSettings>("/accounts/settings") : null]);
    setUsers(values[0]);setGroups(values[1]);if(values[2]) setSettings(values[2]);await auth.refresh();
  },[manageUsers,manageGroups,manageSettings,auth.refresh]);
  useEffect(() => { void perform("Loading accounts",reload); },[perform,reload]);
  return <div className="feature-stack"><div className="page-heading"><div><h1>Admin settings</h1><p>Signed in as {auth.user?.name}. Project access is managed from each project’s Members page.</p></div></div>
    {manageUsers && <UserManagement users={users} groups={groups} catalog={auth.session?.permission_catalog || []} busy={busy} perform={perform} reload={reload} />}
    {manageGroups && <GroupManagement groups={groups} catalog={auth.session?.permission_catalog || []} busy={busy} perform={perform} reload={reload} />}
    {manageSettings && settings && <section className="panel"><div className="panel-heading"><div><h2>Workflow settings</h2><p>These policies apply to all projects.</p></div></div><form className="account-form" onSubmit={(e) => { e.preventDefault();void perform("Saving workflow settings",async () => { await put("/accounts/settings",settings);await reload(); }); }}>
      <label className="check-label"><input type="checkbox" checked={settings.independent_reviewer} disabled={busy} onChange={(e) => setSettings({ ...settings,independent_reviewer:e.target.checked })} />Use an independent reviewer by default</label>
      <p className="field-help">Users with project review permission can explicitly choose themselves for their own submission.</p>
      <p className="field-help">Production dataset versions always use approved revisions. Working copies and review feedback remain separate.</p>
      <label>Session length (hours)<input type="number" min={1} max={168} value={settings.session_hours} onChange={(e) => setSettings({ ...settings,session_hours:Number(e.target.value) })} disabled={busy} /></label><button className="button primary" disabled={busy}>Save settings</button>
    </form></section>}
  </div>;
}
