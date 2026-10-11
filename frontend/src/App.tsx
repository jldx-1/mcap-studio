import { CheckCircle2, LoaderCircle, Upload, X } from "lucide-react";
import Editor from "./components/annotations/AnnotationEditor";
import StructuredEditor from "./components/annotations/StructuredEditor";
import { DatasetWorkspace } from "./components/datasets/DatasetWorkspace";
import { Labels } from "./components/labels/Labels";
import { WorkspaceHeading } from "./components/layout/WorkspaceHeading";
import { WorkspaceSidebar, WorkspaceTopbar } from "./components/layout/WorkspaceNavigation";
import { TopicTable } from "./components/mcaps/TopicTable";
import { Pipeline } from "./components/processing/PipelinePanel";
import { WorkspaceDialogs } from "./components/WorkspaceDialogs";
import { useWorkspace } from "./hooks/useWorkspace";
import { McapsPage } from "./pages/McapsPage";
import { ProjectOverview } from "./pages/ProjectOverview";
import { ProjectsPage } from "./pages/ProjectsPage";
import { AccountPage, PermissionsPage } from "./pages/AccountPage";
import { AdministrationPage } from "./pages/AdministrationPage";
import { ProjectMembers } from "./components/projects/ProjectMembers";
import { ReviewsPage } from "./pages/ReviewsPage";
import { ReviewDetailPage } from "./pages/ReviewDetailPage";
import { AuthProvider, useAuth } from "./state/AuthContext";
import { can } from "./utils/permissions";

export default function App() {
  return <AuthProvider><Workspace /></AuthProvider>;
}
function Workspace() {
  const auth = useAuth();
  const workspace = useWorkspace();
  const {
    page,
    setPage,
    sidebar,
    setSidebar,
    project,
    recording,
    setRecording,
    channel,
    setChannel,
    connected,
    busy,
    error,
    setError,
    notice,
    setNotice,
    datasetStart,
    setDatasetStart,
    workflow,
    setWorkflow,
    uploadRef,
    registerNavigationGuard,
    perform,
    connect,
    chooseRecording,
    go,
    updateProject,
    upload,
    onSaved,
    openDialog
  } = workspace;
  const uploadButton = (
    can(project,auth.user,"project.manage") ?
    <button
      className="button primary"
      disabled={!!busy || connected !== true}
      onClick={() => uploadRef.current?.click()}
    >
      <Upload size={17} /> Load MCAPs
    </button> : null
  );



  return (
    <div className="app-shell">
      <WorkspaceSidebar workspace={workspace} uploadButton={uploadButton} />
      {sidebar && (
        <div className="sidebar-overlay" onClick={() => setSidebar(false)} />
      )}
      <div className="main-shell">
        <WorkspaceTopbar workspace={workspace} />
        <main>
          {error && (
            <div className="alert error" role="alert">
              <div>
                <strong>
                  {connected === false
                    ? "The backend is unavailable"
                    : "Something needs attention"}
                </strong>
                <span>
                  {connected === false
                    ? "Start the workspace with npm run dev from the repository root, then reconnect."
                    : error}
                </span>
              </div>
              {connected === false && (
                <button
                  className="button secondary small"
                  onClick={() => void connect()}
                >
                  Reconnect
                </button>
              )}
              <button
                className="icon-button"
                aria-label="Dismiss error"
                onClick={() => setError("")}
              >
                <X size={17} />
              </button>
            </div>
          )}
          {busy && (
            <div className="progress-banner" role="status">
              <LoaderCircle size={17} className="spin" />
              {busy}…
            </div>
          )}
          <WorkspaceHeading workspace={workspace} uploadButton={uploadButton} />
          {!project && page === "projects" && <ProjectsPage workspace={workspace} />}
          {page === "account" && <AccountPage busy={!!busy} perform={perform} />}
          {page === "permissions" && <PermissionsPage project={project} />}
          {page === "administration" && <AdministrationPage busy={!!busy} perform={perform} />}
          {project && page === "members" && can(project,auth.user,"project.manage") && <ProjectMembers project={project} busy={!!busy} perform={perform} onProject={updateProject} />}
          {project && page === "reviews" && (can(project,auth.user,"review.view") || can(project,auth.user,"annotation.view")) && <ReviewsPage project={project} busy={!!busy} perform={perform} onError={setError} onOpen={(target)=>void workspace.openReview(target)} />}
          {project && page === "review" && (can(project,auth.user,"review.view") || can(project,auth.user,"annotation.view")) && <ReviewDetailPage key={`${project.uuid}/${workspace.reviewTarget?.taskId}`} project={project} target={workspace.reviewTarget} busy={!!busy} perform={perform} onError={setError} onBack={()=>void go("reviews")} />}
          {project && page === "project" && (
            <ProjectOverview project={project} busy={!!busy} onOpen={(value) => { if (value === "datasets") setDatasetStart("load"); void go(value); }} />
          )}
          {project && page === "labels" && can(project,auth.user,"labels.manage") && (
            <Labels project={project} busy={!!busy} perform={perform} onProject={updateProject} onNotice={setNotice} />
          )}
          {project && page === "mcaps" && <McapsPage workspace={workspace} uploadButton={uploadButton} />}
          {project && recording && page === "topics" && (
            <TopicTable
              project={project}
              recording={recording}
              busy={!!busy}
              perform={perform}
              onRecording={setRecording}
              onEdit={(id) => {
                setChannel(id);
                setPage(recording.topics.find((topic) => String(topic.channel_id) === id)?.kind === "image" ? "labeling" : "structured");
              }}
            />
          )}
          {project && recording && page === "structured" && (
            <StructuredEditor key={`${project.uuid}/${recording.uuid}/${channel}`} project={project} recording={recording} channel={channel} perform={perform} busy={!!busy} onProject={updateProject} onNotice={setNotice} onError={setError} onBack={() => void go("topics")} registerNavigationGuard={registerNavigationGuard} />
          )}
          {project && recording && page === "labeling" && (
            <Editor
              readOnly={!can(project,auth.user,"annotation.edit") && !can(project,auth.user,"annotation.create")}
              key={`${project.uuid}/${recording.uuid}/${channel}`}
              project={project}
              recording={recording}
              channel={channel}
              onError={setError}
              onNotice={setNotice}
              onSaved={onSaved}
              registerNavigationGuard={registerNavigationGuard}
              onBack={() => void go("topics")}
              onLabels={() => openDialog("labels")}
            />
          )}
          {project && recording && workflow && page === "pipeline" && (
            <Pipeline
              recording={recording}
              workflow={workflow}
              onChange={setWorkflow}
              busy={!!busy}
              onSave={() => void workspace.saveProcessing()}
              onRun={() => void workspace.runProcessing()}
            />
          )}
          {project && page === "datasets" && (
            <DatasetWorkspace
              key={project.uuid}
              project={project}
              mode={datasetStart}
              onModeChange={setDatasetStart}
              onOpenRecording={chooseRecording}
              busy={!!busy}
              perform={perform}
              onProject={updateProject}
              onNotice={setNotice}
            />
          )}
        </main>
      </div>
      <input
        type="file"
        ref={uploadRef}
        className="visually-hidden"
        aria-label="Load MCAP files"
        accept=".mcap"
        multiple
        onChange={(event) => void upload(Array.from(event.target.files || []))}
      />
      {notice && (
        <div className="toast" role="status">
          <CheckCircle2 size={20} />
          {notice}
          <button
            className="icon-button"
            aria-label="Dismiss notification"
            onClick={() => setNotice("")}
          >
            <X size={16} />
          </button>
        </div>
      )}
      <WorkspaceDialogs workspace={workspace} />





    </div>
  );
}
