import { useState } from "react";
import { PermissionPicker } from "../components/accounts/PermissionPicker";
import { put } from "../services/http";
import { useAuth } from "../state/AuthContext";
import type { Project } from "../types/models";
import type { Perform } from "../types/workspace";

export function AccountPage({ busy,perform }: { busy: boolean;perform: Perform }) {
  const auth=useAuth();const [name,setName]=useState(auth.user?.name || "");
  const [currentPassword,setCurrentPassword]=useState("");const [password,setPassword]=useState("");
  return <div className="feature-stack"><div className="page-heading"><div><h1>Account settings</h1><p>Your profile and sign-in credentials.</p></div></div>
    <section className="panel"><div className="panel-heading"><h2>Profile</h2><span className="pill">{auth.user?.username}</span></div><form className="account-form" onSubmit={(e) => { e.preventDefault();void perform("Saving profile",async () => { await put("/auth/profile",{name});await auth.refresh(); }); }}><label>Display name<input required maxLength={120} value={name} disabled={busy} onChange={(e) => setName(e.target.value)} /></label><button className="button primary" disabled={busy}>Save profile</button></form></section>
    <section className="panel"><div className="panel-heading"><div><h2>Change password</h2><p>This ends all sessions for your account.</p></div></div><form className="account-form" onSubmit={(e) => { e.preventDefault();void perform("Changing password",async () => { await put("/auth/password",{current_password:currentPassword,password});setPassword("");setCurrentPassword("");await auth.refresh(); }); }}><label>Current password<input type="password" autoComplete="current-password" required value={currentPassword} disabled={busy} onChange={(e) => setCurrentPassword(e.target.value)} /></label><label>New password<input type="password" autoComplete="new-password" required minLength={12} maxLength={256} value={password} disabled={busy} onChange={(e) => setPassword(e.target.value)} /></label><button className="button primary" disabled={busy}>Change password</button></form></section>
  </div>;
}
export function PermissionsPage({ project }: { project: Project | null }) {
  const { user,session }=useAuth();
  return <div className="feature-stack"><div className="page-heading"><div><h1>Role and permissions</h1><p>{user?.name} · {user?.group_names?.join(", ") || "No account groups assigned"}</p></div></div>
    <section className="panel"><div className="panel-heading"><div><h2>Account permissions</h2><p>Inherited account grants. An administrator manages these assignments.</p></div></div><div className="collection-body"><PermissionPicker permissions={session?.permission_catalog || []} selected={user?.permissions || []} onChange={() => {}} disabled label="Workspace permissions" /></div></section>
    {project && <section className="panel"><div className="panel-heading"><div><h2>{project.name}</h2><p>Effective permissions for the open project, including project grants and restrictions.</p></div></div><div className="collection-body"><PermissionPicker permissions={(session?.permission_catalog || []).filter((p) => !["users.manage","groups.manage","settings.manage","projects.admin"].includes(p))} selected={project.permissions || []} onChange={() => {}} disabled label="Project permissions" /></div></section>}
  </div>;
}
