import { ChevronUp, LogOut, Monitor, Moon, Settings, Shield, Sun, UserRound } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { WorkspaceController } from "../../hooks/useWorkspace";
import type { Page, Theme } from "../../types/workspace";

type Controls = Pick<WorkspaceController,"user"|"busy"|"theme"|"setTheme"|"go"|"signOut">;
export function AccountMenu({ workspace }: { workspace: Controls }) {
  const { user,busy,theme,setTheme,go,signOut } = workspace;
  const [open,setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null), trigger = useRef<HTMLButtonElement>(null);
  const admin = user?.permissions.some((p) => ["users.manage","groups.manage","settings.manage"].includes(p));
  useEffect(() => {
    const outside = (event: PointerEvent) => { if(!root.current?.contains(event.target as Node)) setOpen(false); };
    const key = (event: KeyboardEvent) => { if(event.key === "Escape") { setOpen(false);trigger.current?.focus(); } };
    if(open) { document.addEventListener("pointerdown",outside);document.addEventListener("keydown",key); }
    return () => { document.removeEventListener("pointerdown",outside);document.removeEventListener("keydown",key); };
  },[open]);
  const navigate = (page: Page) => { setOpen(false);void go(page); };
  const names = user?.name.trim().split(/\s+/) || [];
  const initials = (names[0]?.[0] || "") + (names.length > 1 ? names.at(-1)?.[0] || "" : "");
  return <div className="account-menu" ref={root}>
    {open && <div className="account-popover" id="account-controls" aria-label="Account controls">
      <div className="account-popover-heading"><strong>{user?.name}</strong><span>{user?.username}</span></div>
      <button disabled={!!busy} onClick={() => navigate("account")}><UserRound size={17} />Account settings</button>
      <button disabled={!!busy} onClick={() => navigate("permissions")}><Shield size={17} />Role and permissions</button>
      {admin && <button disabled={!!busy} onClick={() => navigate("administration")}><Settings size={17} />Admin settings</button>}
      <div className="account-appearance"><span>Appearance</span><div className="theme-picker" aria-label="Color theme">{[{value:"light",icon:Sun,label:"Light"},{value:"dark",icon:Moon,label:"Dark"},{value:"system",icon:Monitor,label:"System"}].map((item) => <button key={item.value} aria-label={`${item.label} theme`} aria-pressed={theme===item.value} className={theme===item.value ? "selected" : ""} onClick={() => setTheme(item.value as Theme)}><item.icon size={15} /><span>{item.label}</span></button>)}</div></div>
      <button className="account-sign-out" disabled={!!busy} onClick={() => { setOpen(false);void signOut(); }}><LogOut size={17} />Sign out</button>
    </div>}
    <button ref={trigger} className="account-menu-trigger" aria-label={`Account menu for ${user?.name}`} aria-expanded={open} aria-controls="account-controls" onClick={() => setOpen((value) => !value)}>
      <span className="account-avatar" aria-hidden="true">{initials.toUpperCase()}</span><span><strong>{user?.name}</strong><small>{user?.group_names?.join(" · ") || "Member"}</small></span><ChevronUp size={16} className={open ? "is-open" : ""} />
    </button>
  </div>;
}
