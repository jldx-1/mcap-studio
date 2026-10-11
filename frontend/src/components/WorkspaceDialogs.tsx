import { Database, FolderOpen, Plus } from "lucide-react";
import type { WorkspaceController } from "../hooks/useWorkspace";
import { filename } from "../utils/recordings";
import { ExportAnnotations } from "./annotations/ExportAnnotations";
import { Labels } from "./labels/Labels";
import { ProjectBrowser } from "./projects/ProjectBrowser";
import Dialog from "./shared/Dialog";

export function WorkspaceDialogs({ workspace }: {  
workspace: Pick<WorkspaceController,
    | "project"
    | "recording"
    | "busy"
    | "error"
    | "setNotice"
    | "dialog"
    | "setDialog"
    | "projectName"
    | "setProjectName"
    | "removeTarget"
    | "setRemoveTarget"
    | "setDatasetStart"
    | "canLeave"
    | "perform"
    | "refreshProjects"
    | "showProject"
    | "openProject"
    | "go"
    | "updateProject"
    | "createProject"
    | "removeRecording"
  >
}) {
  const {
    project,
    recording,
    busy,
    error,
    setNotice,
    dialog,
    setDialog,
    projectName,
    setProjectName,
    removeTarget,
    setRemoveTarget,
    setDatasetStart,
    perform,
    openProject,
    go,
    updateProject
  } = workspace;

  return (<>
    {removeTarget && project && (() => {
      const dependencies = project.datasets.filter((dataset) => dataset.source_recording_uuid === removeTarget.uuid || dataset.sources?.some((source) => source.mcap_uuid === removeTarget.uuid));
      return <Dialog title={`Remove ${filename(removeTarget.source_file)}?`} error={error} onClose={() => { if (!busy) setRemoveTarget(null); }}>
        <p>This removes the project copy, its annotations, and cached frames. The original imported file will be kept.</p>
        {!!dependencies.length && <p className="inline-error">Delete the datasets referencing this MCAP first: {dependencies.map((dataset) => dataset.name).join(", ")}.</p>}
        <div className="modal-footer"><button className="button secondary" disabled={!!busy} onClick={() => setRemoveTarget(null)}>Cancel</button><button className="button primary" disabled={!!busy || dependencies.length > 0} onClick={() => void workspace.removeRecording()}>Remove MCAP</button></div>
      </Dialog>;
    })()}
    {dialog === "open" && (
      <ProjectBrowser
        error={error}
        busy={!!busy}
        onOpen={openProject}
        onClose={() => {
          if (!busy) setDialog(null);
        }}
      />
    )}
    {dialog === "create" && (
      <Dialog
        error={error}
        title="Create a project"
        onClose={() => {
          if (!busy) setDialog(null);
        }}
      >
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void workspace.createProject();
          }}
        >
          <label>
            Project name
            <input
              value={projectName}
              onChange={(event) => setProjectName(event.target.value)}
              placeholder="e.g. Warehouse robotics"
              required
            />
          </label>
          <div className="modal-footer">
            <button
              className="button secondary"
              type="button"
              disabled={!!busy}
              onClick={() => setDialog(null)}
            >
              Cancel
            </button>
            <button
              className="button primary"
              disabled={!!busy || !projectName.trim()}
            >
              Create project
            </button>
          </div>
        </form>
      </Dialog>
    )}
    {dialog === "dataset" && (
      <Dialog
        title="Create or load a dataset"
        onClose={() => setDialog(null)}
      >
        <p className="modal-description">
          Choose annotated topics from the MCAPs in {project?.name}, or
          continue working with a saved dataset.
        </p>
        <div className="dataset-options">
          <button
            className="button primary"
            onClick={() => {
              setDialog(null);
              setDatasetStart("new");
              void go("datasets");
            }}
          >
            <Plus size={17} /> Create a visual dataset
          </button>
          <button className="button secondary" onClick={() => { setDialog(null); void go("mcaps"); }}>
            <Database size={17} /> Create CSV from a message topic
          </button>
          <button
            className="button secondary"
            onClick={() => {
              setDialog(null);
              setDatasetStart("load");
              void go("datasets");
            }}
          >
            <FolderOpen size={17} /> Load an existing dataset
          </button>
        </div>
      </Dialog>
    )}
    {dialog === "export" && project && recording && (
      <ExportAnnotations
        error={error}
        project={project}
        recording={recording}
        busy={!!busy}
        perform={perform}
        onNotice={setNotice}
        onClose={() => {
          if (!busy) setDialog(null);
        }}
      />
    )}
    {dialog === "labels" && project && (
      <Dialog
        error={error}
        title="Label lists"
        onClose={() => {
          if (!busy) setDialog(null);
        }}
      >
        <Labels
          project={project}
          busy={!!busy}
          perform={perform}
          onProject={updateProject}
          onNotice={setNotice}
        />
      </Dialog>
    )}
  </>);
}
