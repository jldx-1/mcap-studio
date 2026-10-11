import { useEffect, useState } from "react";
import { api, put } from "../../services/http";
import { useAuth } from "../../state/AuthContext";
import type { Group, ManagedUser, Member } from "../../types/accounts";
import type { Project } from "../../types/models";
import type { Perform } from "../../types/workspace";
import { PermissionPicker } from "../accounts/PermissionPicker";

export function ProjectMembers({ project, busy, perform, onProject }: { project: Project; busy: boolean; perform: Perform; onProject: (p: Project) => Promise<void> }) {
  const auth = useAuth();
  const [members,setMembers] = useState<Member[]>([]);const [users,setUsers] = useState<ManagedUser[]>([]);const [groups,setGroups] = useState<Group[]>([]);
  const [selected,setSelected] = useState("");const [draft,setDraft] = useState<Member | null>(null);
  const base = `/projects/${project.uuid}`;
  useEffect(() => { void perform("Loading project members",async () => { const [m,options] = await Promise.all([api<Member[]>(`${base}/members`),api<{ users:ManagedUser[];groups:Group[] }>(`${base}/access-options`)]);setMembers(m);setUsers(options.users);setGroups(options.groups); }); },[base,perform]);
  const catalog = (auth.session?.permission_catalog || []).filter((p) => !["projects.admin","users.manage","groups.manage","settings.manage"].includes(p));
  const save = (next: Member[]) => void perform("Saving project members",async () => { setMembers(await put<Member[]>(`${base}/members`,{ members:next }));await onProject(await api<Project>(base));setDraft(null);setSelected(""); });
  return <section className="panel"><div className="panel-heading"><div><h1>Project members</h1><p>{project.name} · Group permissions apply only after project access is assigned.</p></div></div>
    <div className="account-management"><div className="account-list">{members.map((m) => <button key={m.user_id} className="recording-row" disabled={busy} onClick={() => { setSelected(m.user_id);setDraft(structuredClone(m)); }}><div><strong>{m.name}</strong><span>{m.enabled ? "Enabled" : "Disabled"} · {m.groups.map((id) => groups.find((g) => g.id === id)?.name).filter(Boolean).join(", ") || "Account groups"}</span></div></button>)}
      <label>Add member<select aria-label="Add project member" value={selected} disabled={busy} onChange={(e) => { setSelected(e.target.value);setDraft(members.find((m) => m.user_id === e.target.value) || { user_id:e.target.value,groups:[],permissions:["project.view"],denied_permissions:[] }); }}><option value="">Select a user…</option>{users.filter((u) => u.enabled).map((u) => <option key={u.id} value={u.id}>{u.name} ({u.username})</option>)}</select></label>
    </div>{draft && <form className="account-form" onSubmit={(e) => { e.preventDefault();save([...members.filter((m) => m.user_id !== draft.user_id),draft]); }}><h2>{users.find((u) => u.id === draft.user_id)?.name}</h2>
      <p className="field-help">Effective access: {(draft.effective_permissions || []).join(", ") || "Save this membership to calculate inherited permissions."}</p>
      <fieldset disabled={busy}><legend>Project groups</legend>{groups.map((g) => <label className="check-label" key={g.id}><input type="checkbox" checked={draft.groups.includes(g.id)} onChange={(e) => setDraft({ ...draft,groups:e.target.checked ? [...draft.groups,g.id] : draft.groups.filter((id) => id !== g.id) })} />{g.name}</label>)}</fieldset>
      <PermissionPicker permissions={catalog} selected={draft.permissions} onChange={(permissions) => setDraft({ ...draft,permissions })} disabled={busy} label="Additional project permissions" />
      <PermissionPicker permissions={catalog} selected={draft.denied_permissions} onChange={(denied_permissions) => setDraft({ ...draft,denied_permissions })} disabled={busy} label="Restricted permissions (override groups)" />
      <div className="button-row"><button className="button primary" disabled={busy}>Save membership</button>{members.some((m) => m.user_id === draft.user_id) && <button type="button" className="button secondary danger" disabled={busy} onClick={() => save(members.filter((m) => m.user_id !== draft.user_id))}>Remove member</button>}</div>
    </form>}</div>
  </section>;
}
