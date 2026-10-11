import { useState } from "react";
import { api, post, put } from "../../services/http";
import type { Group } from "../../types/accounts";
import type { Perform } from "../../types/workspace";
import { PermissionPicker } from "./PermissionPicker";

export function GroupManagement({ groups, catalog, busy, perform, reload }: { groups: Group[]; catalog: string[]; busy: boolean; perform: Perform; reload: () => Promise<void> }) {
  const [id, setId] = useState("");const [name, setName] = useState("");const [permissions, setPermissions] = useState<string[]>([]);
  const select = (group?: Group) => { setId(group?.id || "");setName(group?.name || "");setPermissions(group?.permissions || []); };
  return <section className="panel"><div className="panel-heading"><div><h2>Groups</h2><p>Define roles by selecting their permissions.</p></div><button className="button secondary small" disabled={busy} onClick={() => select()}>New group</button></div>
    <div className="account-management"><div className="account-list">{groups.map((g) => <button key={g.id} className="recording-row" disabled={busy} onClick={() => select(g)}><div><strong>{g.name}</strong><span>{g.permissions.length} permissions</span></div></button>)}</div>
      <form className="account-form" onSubmit={(e) => { e.preventDefault();void perform("Saving group",async () => { if(id) await put(`/accounts/groups/${id}`,{ name,permissions });else await post("/accounts/groups",{ name,permissions });await reload();select(); }); }}>
        <h3>{id ? "Edit group" : "Create group"}</h3><label>Group name<input required value={name} onChange={(e) => setName(e.target.value)} disabled={busy} maxLength={120} /></label>
        <PermissionPicker permissions={catalog} selected={permissions} onChange={setPermissions} disabled={busy} />
        <div className="button-row"><button className="button primary" disabled={busy}>Save group</button>{id && <button type="button" className="button secondary danger" disabled={busy} onClick={() => { if(window.confirm(`Remove ${name}? Users will lose permissions inherited from this group.`)) void perform("Removing group",async () => { await api(`/accounts/groups/${id}`,{ method:"DELETE" });await reload();select(); }); }}>Remove group</button>}</div>
      </form>
    </div>
  </section>;
}
