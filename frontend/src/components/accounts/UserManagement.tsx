import { useState } from "react";
import { api, post, put } from "../../services/http";
import type { Group, ManagedUser } from "../../types/accounts";
import type { Perform } from "../../types/workspace";
import { UserProjectAssignments } from "./UserProjectAssignments";
import Dialog from "../shared/Dialog";
import { PermissionPicker } from "./PermissionPicker";

export function UserManagement({ users, groups, catalog, busy, perform, reload }: { users: ManagedUser[]; groups: Group[]; catalog: string[]; busy: boolean; perform: Perform; reload: () => Promise<void> }) {
  const [deleteError,setDeleteError]=useState("");
  const [deleting,setDeleting] = useState<ManagedUser | null>(null);
  const [id, setId] = useState("");
  const [username, setUsername] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [selectedGroups, setGroups] = useState<string[]>([]);
  const [permissions, setPermissions] = useState<string[]>([]);
  const select = (user?: ManagedUser) => { setId(user?.id || "");setUsername(user?.username || "");setName(user?.name || "");setPassword("");setEnabled(user?.enabled ?? true);setGroups(user?.groups || []);setPermissions(user?.permissions || []); };
  return <section className="panel">{deleting && <Dialog title={`Delete ${deleting.name}?`} error={deleteError} onClose={() => { if(!busy) setDeleting(null); }}><p>This removes sign-in access and permissions. Their annotations, submitted revisions, comments, approvals, and recorded names remain in history.</p><p>The last administrator cannot be deleted.</p><div className="modal-footer"><button className="button secondary" disabled={busy} onClick={() => setDeleting(null)}>Cancel</button><button className="button primary danger" disabled={busy} onClick={() => void perform("Deleting user",async () => { try{await api(`/accounts/users/${deleting.id}`,{method:"DELETE"});}catch(err){setDeleteError(err instanceof Error?err.message:String(err));throw err;}setDeleting(null);select();await reload(); })}>Delete user</button></div></Dialog>}<div className="panel-heading"><div><h2>Users</h2><p>Create accounts, assign groups, reset passwords, or disable access.</p></div><button className="button secondary small" onClick={() => select()} disabled={busy}>New user</button></div>
    <div className="account-management"><div className="account-list">{users.map((u) => <button key={u.id} className={`recording-row ${id === u.id ? "selected" : ""}`} onClick={() => select(u)} disabled={busy}><div><strong>{u.name}</strong><span>{u.username} · {u.enabled ? "Enabled" : "Disabled"}</span></div></button>)}</div>
      <form className="account-form" onSubmit={(e) => { e.preventDefault();void perform("Saving user",async () => { const body = { username, name, password, enabled, groups: selectedGroups, permissions }; const updated = id ? await put<ManagedUser>(`/accounts/users/${id}`,body) : await post<ManagedUser>("/accounts/users",body);setPassword("");await reload();select(updated); }); }}>
        <h3>{id ? "Edit account" : "Create account"}</h3><label>Name<input required value={name} onChange={(e) => setName(e.target.value)} maxLength={120} disabled={busy} /></label><label>Username<input required value={username} onChange={(e) => setUsername(e.target.value)} disabled={busy || !!id} maxLength={120} autoComplete="off" /></label>
        <label>{id ? "New password (leave blank to keep it)" : "Temporary password"}<input type="password" required={!id} minLength={12} maxLength={256} value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" disabled={busy} /></label>
        <label className="check-label"><input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} disabled={busy} />Account enabled</label>
        <fieldset disabled={busy}><legend>Groups</legend>{groups.map((g) => <label key={g.id} className="check-label"><input type="checkbox" checked={selectedGroups.includes(g.id)} onChange={(e) => setGroups(e.target.checked ? [...selectedGroups,g.id] : selectedGroups.filter((id) => id !== g.id))} />{g.name}</label>)}</fieldset>
        <PermissionPicker permissions={catalog} selected={permissions} onChange={setPermissions} disabled={busy} label="Additional permissions" />
        <p className="field-help">Project membership is required unless this user has projects.admin. Saving an existing account ends its active sessions.</p><div className="button-row"><button className="button primary" disabled={busy}>Save user</button>{id && <button type="button" className="button secondary danger" disabled={busy} onClick={() => {setDeleteError("");setDeleting(users.find((u) => u.id === id) || null);}}>Delete user…</button>}</div>
      </form>
      {id && <UserProjectAssignments key={id} userId={id} busy={busy} perform={perform} />}
    </div>
  </section>;
}
