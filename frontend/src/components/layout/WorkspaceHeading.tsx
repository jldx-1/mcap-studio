import { ArrowLeft, Database, FolderOpen, Plus } from "lucide-react";
import type { ReactNode } from "react";
import type { WorkspaceController } from "../../hooks/useWorkspace";
import { can } from "../../utils/permissions";
import { filename } from "../../utils/recordings";

export function WorkspaceHeading({ workspace, uploadButton }: {  
workspace: Pick<WorkspaceController,
    | "user"
    | "page"
    | "project"
    | "recording"
    | "channel"
    | "connected"
    | "busy"
    | "setProjectName"
    | "datasetStart"
    | "setDatasetStart"
    | "go"
    | "openDialog"
  >; uploadButton: ReactNode
}) {
  const {
    user,
    page,
    project,
    recording,
    channel,
    connected,
    busy,
    setProjectName,
    datasetStart,
    setDatasetStart,
    go,
    openDialog
  } = workspace;

  if(["reviews","review","members","administration","account","permissions"].includes(page)) return null;
  const selectedTopic = recording?.topics.find(
    (topic) => String(topic.channel_id) === channel,
  );
  const title = !project
    ? "Load a project to begin"
    : page === "project"
      ? project.name
      : page === "mcaps"
        ? "Recordings"
        : page === "labels"
          ? "Labels & Translations"
          : page === "datasets"
            ? "Project datasets"
            : page === "prepare" ? "Prepare recordings" : page === "pipeline"
              ? "Prepare channels"
              : (page === "labeling" || page === "structured")
                ? selectedTopic?.topic || "Label topic"
                : filename(recording?.source_file || "MCAP topics");
  const description = !project
    ? "Choose an existing project or browse your computer for a project folder."
    : page === "project"
      ? "Explore your recordings, labeling progress, and datasets."
      : page === "mcaps"
        ? "Browse recordings by import date and open their topic tables."
        : page === "labels"
          ? "Manage reusable label lists and translation scripts for your recordings."
          : page === "topics"
            ? "Choose a labeling format and label list, then create or edit labels beside a topic."
            : page === "structured"
              ? "Convert message fields into columns, add truth values, and save a CSV dataset."
              : page === "labeling"
                ? "Label frames and submit a completed task for review."
                : page === "datasets"
                  ? "Select approved annotations from the MCAPs in this project."
                  : "Select, translate, and synchronize channels into a new recording.";
  return (<div className="page-heading">
    <div>
      <div className="eyebrow">
        {!project
          ? "PROJECTS"
          : page === "project"
            ? "PROJECT WORKSPACE"
            : page === "structured"
              ? "STRUCTURED DATA"
              : page === "labels"
                ? "LABELS & TRANSLATIONS"
                : page === "labeling"
                  ? "TOPIC LABELING"
                  : page === "datasets"
                    ? "DATASETS"
                    : "MCAP WORKSPACE"}
      </div>
      <h1>{title}</h1>
      <p>{description}</p>
    </div>
    <div className="button-row">
      {!project && can(null,user,"projects.admin") && (
        <>
          <button
            className="button primary"
            disabled={!!busy || connected !== true}
            onClick={() => openDialog("open")}
          >
            <FolderOpen size={17} /> Browse server
          </button>
          <button
            className="button secondary"
            disabled={!!busy || connected !== true}
            onClick={() => {
              setProjectName("");
              openDialog("create");
            }}
          >
            <Plus size={17} /> New project
          </button>
        </>
      )}
      {page === "project" && (
        <>
          {can(project,user,"datasets.generate") && <button
            className="button secondary"
            disabled={!!busy}
            onClick={() => openDialog("dataset")}
          >
            <Database size={17} /> Create dataset
          </button>}
          {uploadButton}
        </>
      )}
      {(page === "mcaps" || page === "labels" || page === "topics" ||
        page === "pipeline" ||
        page === "datasets") && (
          <button
            className="button secondary"
            disabled={!!busy}
            onClick={() => {
              if (page === "datasets" && datasetStart !== "load") {
                setDatasetStart(datasetStart === "add" ? "view" : "load");
                return;
              }
              void go(page === "topics" ? "mcaps" : "project");
            }}
          >
            <ArrowLeft size={17} /> {page === "topics" ? "Back to MCAPs" : page === "datasets" && datasetStart !== "load" ? datasetStart === "add" ? "Back to dataset" : "Back to datasets" : "Back to project"}
          </button>
        )}
      {page === "pipeline" && (
        <button
          className="button secondary"
          disabled={!!busy}
          onClick={() => void go("topics")}
        >
          <ArrowLeft size={17} /> Back To Table
        </button>
      )}
    </div>
  </div>);
}
