import { ArrowRight, FolderOpen } from "lucide-react";
import { Empty } from "../components/shared/Empty";
import type { WorkspaceController } from "../hooks/useWorkspace";

export function ProjectsPage({ workspace }: { workspace: Pick<WorkspaceController, "projects" | "connected" | "busy" | "search" | "setSearch" | "loadProject"> }) {
  const { projects, connected, busy, search, setSearch, loadProject } = workspace;

  return (<section className="panel">
    <div className="panel-heading">
      <div>
        <h2>Available projects</h2>
        <p>Open a project, or drop MCAP files here and choose where to import them.</p>
      </div>
      <span className="pill">{projects.length} projects</span>
    </div>
    {!!projects.length && (
      <div className="collection-body">
        <input
          aria-label="Search projects"
          placeholder="Search projects by name or location"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
      </div>
    )}
    {projects
      .filter((item) =>
        `${item.name} ${item.path || ""}`
          .toLowerCase()
          .includes(search.toLowerCase()),
      )
      .map((item) => (
        <button
          className="recording-row"
          key={item.uuid}
          disabled={!!busy || connected !== true}
          onClick={() => void loadProject(item.uuid)}
        >
          <span className="file-icon">
            <FolderOpen size={21} />
          </span>
          <div>
            <strong>{item.name}</strong>
            <span>
              {item.path ? `${item.path} · ` : ""}
              {item.recording_count} MCAPs · {item.dataset_count}{" "}
              datasets
            </span>
          </div>
          <ArrowRight size={17} />
        </button>
      ))}
    {!projects.length && (
      <Empty
        icon={<FolderOpen size={28} />}
        title="Find your project"
      >
        Projects assigned to your account appear here. Administrators can browse for an existing project or create one.
      </Empty>
    )}
    {!!projects.length &&
      !projects.some((item) =>
        `${item.name} ${item.path || ""}`
          .toLowerCase()
          .includes(search.toLowerCase()),
      ) && (
        <Empty
          icon={<FolderOpen size={28} />}
          title="No matching projects"
        >
          Try another name or browse the server.
        </Empty>
      )}
  </section>);
}
