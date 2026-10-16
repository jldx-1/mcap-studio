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
import { useFileDrop } from "./hooks/useFileDrop";
import { RecordingPreparation } from "./components/processing/RecordingPreparation";
import { PreparationJobs } from "./components/processing/PreparationJobs";
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
  const dropEnabled = !busy && connected === true && (!project || can(project, auth.user, "project.manage"));
  const draggingFiles = useFileDrop(dropEnabled, upload);
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
          {!!workspace.pendingUploads.length && (
            <div className="pending-import-banner" role="status">
              <Upload size={20} />
              <div>
                <strong>{workspace.pendingUploads.length} file{workspace.pendingUploads.length === 1 ? "" : "s"} ready to import</strong>
                <span>{project && can(project, auth.user, "project.manage") ? `Import these files into ${project.name}, or choose another project.` : "Open or create a project you can manage to import these files."}</span>
              </div>
              {project && can(project, auth.user, "project.manage") && <button className="button primary small" disabled={!!busy || connected !== true} onClick={() => void upload(workspace.pendingUploads)}>Import files</button>}
              <button className="button secondary small" disabled={!!busy} onClick={workspace.clearPendingUploads}>Cancel import</button>
            </div>
          )}
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
              {workspace.importProgress || busy}…
            </div>
          )}
          <WorkspaceHeading workspace={workspace} uploadButton={uploadButton} />
          {project && <PreparationJobs project={project} busy={!!busy} perform={perform} onJobs={workspace.resolveLabelingJobs} onProject={updateProject} onLabel={id => void workspace.startLabeling(id, [...new Set([...workspace.labelingQueue, id])])} canManage={can(project, auth.user, "project.manage")} />}
          {project && workspace.preparationRecordings.length > 0 && page !== "prepare" && <div className="pending-import-banner"><span>{workspace.preparationRecordings.length} recordings ready to prepare</span><button className="button secondary small" disabled={!!busy} onClick={() => void go("prepare")}>Prepare recordings</button></div>}
          {project && workspace.labelingQueue.length > 0 && !["labeling", "structured", "prepare"].includes(page) && <div className="pending-import-banner"><span>{workspace.labelingQueue.length} recordings in your labeling queue</span><button className="button primary small" disabled={!!busy} onClick={() => void workspace.startLabeling(workspace.labelingQueue[0])}>Continue labeling queue</button><button className="text-button" disabled={!!busy} onClick={() => workspace.saveLabelingQueue([])}>Clear queue</button></div>}
          {project && page === "prepare" && <RecordingPreparation project={project} recordings={workspace.preparationRecordings} translations={workspace.translationFiles} busy={!!busy} perform={perform} onNotice={setNotice} onQueued={workspace.preparationQueued} onCancel={() => void go("mcaps")} />}
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
              key={recording.uuid}
              onApply={workspace.applyTranslations}
              onNotice={setNotice}
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
            <StructuredEditor key={`${project.uuid}/${recording.uuid}/${channel}`} project={project} recording={recording} channel={channel} perform={perform} busy={!!busy} onProject={updateProject} onNotice={setNotice} onError={setError} onBack={() => void go("topics")} onNext={workspace.nextLabeling} registerNavigationGuard={registerNavigationGuard} />
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
              onNext={() => workspace.nextLabeling()}
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
              onContinueLabeling={(id, source) => workspace.startLabeling(id, [id], source)}
              busy={!!busy}
              perform={perform}
              onProject={updateProject}
              onNotice={setNotice}
              initialRecordings={workspace.datasetRecordingIds}
              onInitialRecordingsUsed={workspace.clearDatasetRecordingIds}
            />
          )}
        </main>
      </div>
      <input
        type="file"
        ref={uploadRef}
        className="visually-hidden"
        aria-label="Load MCAP files"
        accept=".mcap,.json,.txt"
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
      {draggingFiles && (
        <div className={`file-drop-overlay ${dropEnabled ? "" : "unavailable"}`} role="status">
          <div className="file-drop-card">
            <Upload size={40} />
            <strong>{dropEnabled ? "Drop MCAPs and translation files" : "MCAP import is unavailable"}</strong>
            <span>{busy ? "Wait for the current task to finish." : connected !== true ? "Connect to the backend first." : !project ? "Then open or create a project for these files." : !dropEnabled ? "You need permission to manage this project." : `Add recordings to ${project.name}.`}</span>
          </div>
        </div>
      )}
    </div>
  );
}
