import { ArrowRight, FolderOpen, Layers3, Trash2, Upload } from "lucide-react";
import type { ReactNode } from "react";
import { Empty } from "../components/shared/Empty";
import type { WorkspaceController } from "../hooks/useWorkspace";
import { can } from "../utils/permissions";
import { filename, formatDuration } from "../utils/recordings";
import { useReviewTasks } from "../hooks/useReviewTasks";
import { latestRecordingTask, workflowStatus } from "../utils/reviews";
import { statusLabel } from "../types/reviews";

export function McapsPage({ workspace, uploadButton }: {  
workspace: Pick<WorkspaceController,
    | "user"
    | "project"
    | "busy"
    | "setError"
    | "search"
    | "setSearch"
    | "dateSort"
    | "setDateSort"
    | "setRemoveTarget"
    | "chooseRecording"
    | "upload"
    | "openReview"
  >; uploadButton: ReactNode
}) {
  const {
    user,
    project,
    busy,
    setError,
    search,
    setSearch,
    dateSort,
    setDateSort,
    setRemoveTarget,
    chooseRecording,
    upload,
    openReview
  } = workspace;
  const reviewAccess = can(project,user,"annotation.view") || can(project,user,"review.view");
  const reviews = useReviewTasks(project?.uuid || "",setError,!!project && reviewAccess);
  if (!project) return null;

  return (<>
    <section
      className="panel"
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
        if(can(project,user,"project.manage")) void upload(Array.from(event.dataTransfer.files));
      }}
    >
      <div className="panel-heading">
        <div>
          <h2>Project MCAPs</h2>
          <p>Select a recording to open its data labeling table.</p>
        </div>
        <span className="pill">{project.mcaps.length} MCAPs</span>
      </div>
      {!!project.mcaps.length && (
        <div className="collection-body recording-filters">
          <label>Sort by import date<select aria-label="Sort MCAPs by import date" value={dateSort} onChange={(event) => setDateSort(event.target.value)}><option value="newest">Newest first</option><option value="oldest">Oldest first</option></select></label>
          <input
            aria-label="Search included MCAPs"
            placeholder="Search recordings"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
        </div>
      )}
      {[...project.mcaps]
        .sort((a, b) => {
          const aDate = Date.parse(a.created_at || "");
          const bDate = Date.parse(b.created_at || "");
          if (!Number.isFinite(aDate)) return Number.isFinite(bDate) ? 1 : filename(a.source_file).localeCompare(filename(b.source_file));
          if (!Number.isFinite(bDate)) return -1;
          return dateSort === "newest" ? bDate - aDate : aDate - bDate;
        })
        .filter((item) =>
          filename(item.source_file)
            .toLowerCase()
            .includes(search.toLowerCase()),
        )
        .map((item) => {
          const task = latestRecordingTask(reviews.tasks,item.uuid);
          return <div className="mcap-list-card" key={item.uuid}>
          <div className="dataset-list-item" key={item.uuid}>
            <button
              className="recording-row"
              disabled={!!busy}
              onClick={() => void chooseRecording(item.uuid)}
            >
              <span className="file-icon">
                <Layers3 size={21} />
              </span>
              <div>
                <strong>{filename(item.source_file)}</strong>
                <span>{item.created_at ? `Imported ${new Date(item.created_at).toLocaleString()}` : "Import date unavailable"} · {item.summary ? `${item.summary.topic_count} topics · ${formatDuration(item.summary.duration_seconds)} · ${item.summary.frame_count} frames` : "Metrics available after indexing"}</span>
              </div>
              <span className="pill">Open topic table</span>
              <ArrowRight size={17} />
            </button>
            {reviewAccess && (reviews.loading ? <span className="pill mcap-review-status" role="status">Loading status…</span> : task?.current_revision ?
              <button className={`pill mcap-review-status ${workflowStatus(task) === "approved" ? "green" : workflowStatus(task) === "changes_requested" ? "amber" : "blue"}`}
                aria-label={`Review status for ${filename(item.source_file)}: ${statusLabel[workflowStatus(task)]}`}
                title="Open this review" disabled={!!busy} onClick={() => void openReview({taskId:task.id,view:"review"})}>{statusLabel[workflowStatus(task)]}</button>
              : <span className="pill mcap-review-status">Not submitted</span>)}
            <button className="icon-button danger-text" title="Remove MCAP from project" aria-label={`Remove ${filename(item.source_file)} from project`} disabled={!!busy || !can(project,user,"project.manage")} onClick={() => { setError(""); setRemoveTarget(item); }}><Trash2 size={17} /></button>
          </div>
          </div>;
        })}
      {!project.mcaps.length && (
        <Empty
          icon={<Upload size={28} />}
          title="Load your first MCAP"
          action={uploadButton}
        >
          Choose existing .mcap files from your computer or drop them
          here to add them to this project.
        </Empty>
      )}
      {!!project.mcaps.length &&
        !project.mcaps.some((item) =>
          filename(item.source_file)
            .toLowerCase()
            .includes(search.toLowerCase()),
        ) && (
          <Empty
            icon={<Layers3 size={28} />}
            title="No matching MCAPs"
          >
            Try another recording name.
          </Empty>
        )}
    </section>
    <div className="source-note">
      <FolderOpen size={16} />
      <span>
        Project: <code>{project.path}</code>
      </span>
    </div>
  </>);
}
