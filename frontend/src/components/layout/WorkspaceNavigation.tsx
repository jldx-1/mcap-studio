import { Activity, ArrowLeft, ClipboardCheck, Users, Database, FolderOpen, Layers3, Menu, Tag } from "lucide-react";
import { Notifications } from "../notifications/Notifications";
import { AccountMenu } from "../accounts/AccountMenu";
import type { ReactNode } from "react";
import type { WorkspaceController } from "../../hooks/useWorkspace";
import type { Page } from "../../types/workspace";
import { can } from "../../utils/permissions";
import { filename } from "../../utils/recordings";

export function WorkspaceSidebar({ workspace, uploadButton }: { workspace: Pick<WorkspaceController, "user" | "signOut" | "theme" | "setTheme" | "page" | "sidebar" | "project" | "busy" | "setDatasetStart" | "go" | "openDialog">; uploadButton: ReactNode }) {
  const { user,page, sidebar, project, busy, setDatasetStart, go, openDialog } = workspace;

  return (<aside className={`sidebar ${sidebar ? "is-open" : ""}`}>
    <a
      className="brand"
      href="#"
      onClick={(event) => {
        event.preventDefault();
        void go("projects");
      }}
    >
      <span className="brand-mark">
        <Activity size={26} strokeWidth={2.5} />
      </span>
      <span>
        MCAP <b>Studio</b>
        <small>DATA WORKSPACE</small>
      </span>
    </a>
    {project ? (
      <>
        <div className="sidebar-project"><span className="workspace-label">PROJECT</span><strong>{project.name}</strong></div>
        <nav aria-label="Project navigation">
          {[
            { value: "project", label: "Overview", icon: FolderOpen },
            { value: "mcaps", label: "MCAP Library", icon: Layers3,permission:"mcap.view" },
            { value: "labels", label: "Labels & Translations", icon: Tag,permission:"labels.manage" },
            { value:"reviews",label:"Reviews",icon:ClipboardCheck,permission:"review.view" },
            { value: "datasets", label: "Datasets", icon: Database },
            { value:"members",label:"Members",icon:Users,permission:"project.manage" },
          ].filter((item) => !item.permission || can(project,user,item.permission) || (item.value==="labels"&&can(project,user,"project.manage")) || (item.value==="reviews"&&can(project,user,"annotation.view"))).map((item) => (
            <button key={item.value} className={`nav-item ${page === item.value || (item.value === "reviews" && page === "review") || (item.value === "mcaps" && ["topics", "labeling", "structured", "pipeline", "prepare"].includes(page)) ? "active" : ""}`}
              disabled={!!busy} onClick={() => { if (item.value === "datasets") setDatasetStart("load"); void go(item.value as Page); }}>
              <item.icon size={19} /><span>{item.label}</span>
            </button>
          ))}
        </nav>
        <div className="sidebar-project-actions">
          {page!=="mcaps"&&uploadButton}
          {page!=="mcaps"&&can(project,user,"datasets.generate") && <button className="button secondary" disabled={!!busy} onClick={() => openDialog("dataset")}><Database size={17} /> Create dataset</button>}
          <button className="text-button" disabled={!!busy} onClick={() => void go("projects")}><ArrowLeft size={15} /> All projects</button>
        </div>
      </>
    ) : (
      <nav aria-label="Main navigation"><button className={`nav-item ${page === "projects" ? "active" : ""}`} disabled={!!busy} onClick={() => void go("projects")}><FolderOpen size={19} /> Projects</button></nav>
    )}
    <div className="sidebar-bottom"><AccountMenu workspace={workspace} /></div>
  </aside>);
}

export function WorkspaceTopbar({ workspace }: { workspace: WorkspaceController }) {
  const { page, sidebar, setSidebar, project, recording, connected, busy, go } = workspace;

  return (<header className="topbar">
    <div className="topbar-start">
      <button
        className="icon-button mobile-menu"
        aria-label="Toggle navigation"
        onClick={() => setSidebar(!sidebar)}
      >
        <Menu size={22} />
      </button>
      <nav className="breadcrumb" aria-label="Breadcrumb">
        <button disabled={!!busy} onClick={() => void go("projects")}>
          Projects
        </button>
        {project && (
          <>
            <span>/</span>
            <button disabled={!!busy} onClick={() => void go("project")}>
              {project.name}
            </button>
          </>
        )}
        {recording && (
          <>
            <span>/</span>
            <button disabled={!!busy} onClick={() => void go("topics")}>
              {filename(recording.source_file)}
            </button>
          </>
        )}
        {(page === "labeling" || page === "structured") && (
          <>
            <span>/</span>
            <strong>{page === "structured" ? "Message columns" : "Label topic"}</strong>
          </>
        )}
        {page === "datasets" && (
          <>
            <span>/</span>
            <strong>Datasets</strong>
          </>
        )}
      </nav>
    </div>
    <div className="topbar-status">{project&&<Notifications key={`${workspace.user?.id}/${project.uuid}`} workspace={workspace} project={project}/>}<span
      className={`connection ${connected === false ? "offline" : ""}`}
    >
      <span />
      {connected === null
        ? "Connecting"
        : connected
          ? "Backend connected"
          : "Backend offline"}
    </span></div>
  </header>);
}
