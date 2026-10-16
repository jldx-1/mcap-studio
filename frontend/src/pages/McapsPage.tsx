import { useState, type ReactNode } from "react";
import { ArrowRight, FolderOpen, Layers3, Trash2, Upload } from "lucide-react";
import { Empty } from "../components/shared/Empty";
import { post } from "../services/http";
import type { Project } from "../types/models";
import { ServerRecordings } from "../components/processing/ServerRecordings";
import type { WorkspaceController } from "../hooks/useWorkspace";
import type { RecordingSummary } from "../types/models";
import { can } from "../utils/permissions";
import { filename, formatDuration } from "../utils/recordings";
import { useReviewTasks } from "../hooks/useReviewTasks";
import { latestRecordingTask, workflowStatus } from "../utils/reviews";
import { statusLabel } from "../types/reviews";

export function McapsPage({ workspace, uploadButton }: { workspace: WorkspaceController; uploadButton: ReactNode }) {
  const { user, project, busy, setError, search, setSearch, dateSort, setDateSort, setRemoveTarget, chooseRecording, openReview } = workspace;
  const [selected, setSelected] = useState<string[]>([]);
  const [server, setServer] = useState(false);
  const [sessionName, setSessionName] = useState("");
  const reviewAccess = can(project, user, "annotation.view") || can(project, user, "review.view");
  const reviews = useReviewTasks(project?.uuid || "", setError, !!project && reviewAccess);
  if (!project) return null;
  const manage = can(project, user, "project.manage");
  const generate = can(project, user, "datasets.generate");
  const sorted = [...project.mcaps].sort((a, b) => (dateSort === "newest" ? -1 : 1) * ((Date.parse(a.created_at || "") || 0) - (Date.parse(b.created_at || "") || 0)));
  const familyRoot = (recording: RecordingSummary) => {
    let current = recording; const visited = new Set<string>();
    while (current.source_recording_uuid && !visited.has(current.uuid)) {
      visited.add(current.uuid); const parent = project.mcaps.find(r => r.uuid === current.source_recording_uuid);
      if (!parent) break; current = parent;
    }
    return current.uuid;
  };
  const children = (id: string): RecordingSummary[] => sorted.filter(r => r.uuid !== id && familyRoot(r) === id);
  const matches = (r: RecordingSummary) => filename(r.source_file).toLowerCase().includes(search.toLowerCase()) || children(r.uuid).some(child => filename(child.source_file).toLowerCase().includes(search.toLowerCase()));
  const roots = sorted.filter(r => familyRoot(r) === r.uuid).filter(matches);
  const row = (item: RecordingSummary, original = false) => {
    const task = latestRecordingTask(reviews.tasks, item.uuid);
    const members = project.datasets.filter(d => d.sources?.some(s => s.mcap_uuid === item.uuid) || d.source_recording_uuid === item.uuid || d.all_source_recordings?.includes(item.uuid));
    return <div className="mcap-list-card" key={item.uuid}><div className="dataset-list-item">
      <label className="check-label recording-select"><input type="checkbox" aria-label={`Select recording ${filename(item.source_file)}`} checked={selected.includes(item.uuid)} disabled={!!busy} onChange={e => setSelected(current => e.target.checked ? [...current, item.uuid] : current.filter(id => id !== item.uuid))} /></label>
      <button className="recording-row" disabled={!!busy} onClick={() => void chooseRecording(item.uuid)}><span className="file-icon"><Layers3 size={21} /></span><div><strong>{filename(item.source_file)}</strong><span>{original ? "Original source" : item.source_recording_uuid ? "Prepared recording" : "Imported recording"}{item.created_at ? ` · ${new Date(item.created_at).toLocaleString()}` : ""}{item.summary ? ` · ${item.summary.topic_count} topics · ${formatDuration(item.summary.duration_seconds)} · ${item.summary.frame_count} frames` : " · Inspect topics to build previews"}</span>{item.summary?.start_time_ns && Number(item.summary.start_time_ns) > 1e17 && <span>Captured {new Date(Number(item.summary.start_time_ns) / 1e6).toLocaleString()}</span>}{item.session_id && <span>Session: {item.session_id}</span>}{item.summary && <span>{item.summary.annotated_count} labeled · {item.summary.reviewed_count} complete</span>}{members.length > 0 && <span>Datasets: {members.map(d => d.name).join(", ")}</span>}</div><ArrowRight size={17} /></button>
      {reviewAccess && task?.current_revision ? <button className={`pill mcap-review-status ${workflowStatus(task) === "approved" ? "green" : "blue"}`} disabled={!!busy} onClick={() => void openReview({ taskId: task.id, view: "review" })}>{statusLabel[workflowStatus(task)]}</button> : <span className="pill">Not submitted</span>}
      <button className="icon-button danger-text" title="Remove recording from project" aria-label={`Remove ${filename(item.source_file)} from project`} disabled={!!busy || !manage} onClick={() => { setError(""); setRemoveTarget(item); }}><Trash2 size={17} /></button>
    </div><div className="recording-card-actions"><button className="button primary small" disabled={!!busy} onClick={() => task && workflowStatus(task) === "approved" && generate ? void workspace.addRecordingsToDataset([item.uuid]) : !item.summary && manage ? void workspace.prepareRecordings([item.uuid]) : void workspace.startLabeling(item.uuid, [...new Set([...workspace.labelingQueue, item.uuid])])}>{task && workflowStatus(task) === "approved" && generate ? "Add approved data to dataset" : !item.summary && manage ? "Prepare recording" : task && workflowStatus(task) === "changes_requested" ? "Address review feedback" : "Continue labeling"}</button>{manage && <button className="button secondary small" disabled={!!busy} onClick={() => void workspace.prepareRecordings([item.uuid])}>Prepare recording</button>}{generate && <button className="button secondary small" disabled={!!busy} onClick={() => void workspace.addRecordingsToDataset([item.uuid])}>Add to dataset</button>}</div></div>;
  };
  return <>
    <section className="panel"><div className="panel-heading"><div><h2>Recording library</h2><p>Drop MCAPs and translation files, reopen existing recordings, or prepare a labeling queue.</p></div><div className="button-row"><span className="pill">{project.mcaps.length} recordings</span>{manage && <button className="button secondary small" disabled={!!busy} onClick={() => setServer(true)}><FolderOpen size={16} />Load from server</button>}</div></div>
      {!!project.mcaps.length && <div className="collection-body recording-filters"><label>Sort by import date<select aria-label="Sort MCAPs by import date" value={dateSort} onChange={e => setDateSort(e.target.value)}><option value="newest">Newest first</option><option value="oldest">Oldest first</option></select></label><input aria-label="Search included MCAPs" placeholder="Search recordings" value={search} onChange={e => setSearch(e.target.value)} /></div>}
      {!!selected.length && <div className="collection-body button-row"><strong>{selected.length} selected</strong>{manage && <button className="button secondary small" disabled={!!busy} onClick={() => void workspace.prepareRecordings(selected)}>Prepare selected</button>}<button className="button secondary small" disabled={!!busy} onClick={() => void workspace.startLabeling(selected[0], selected)}>Label selected recordings</button>{generate && <button className="button primary small" disabled={!!busy} onClick={() => void workspace.addRecordingsToDataset(selected)}>Add selected to dataset</button>}<button className="text-button" disabled={!!busy} onClick={() => setSelected([])}>Clear selection</button></div>}
      {selected.length > 0 && manage && <div className="collection-body approved-filter-controls"><label>Capture session<input value={sessionName} onChange={e => setSessionName(e.target.value)} placeholder="e.g. Warehouse run 2026-10-14" maxLength={120}/></label><button className="button secondary small" disabled={!!busy || !sessionName.trim()} onClick={() => void workspace.perform("Grouping capture session", async () => { await workspace.updateProject(await post<Project>(`/projects/${project.uuid}/recordings/session`, {recordings:selected,session_id:sessionName.trim()})); workspace.setNotice("Capture session saved. Future dataset versions keep these recordings in the same split."); })}>Group selected as one session</button></div>}
      {roots.map(root => {
        const variants = children(root.uuid); const latest = variants[0];
        return variants.length ? <div key={root.uuid} className="recording-family">{row(latest)}<details><summary>Original and other prepared versions · {variants.length + 1} recordings</summary>{row(root, true)}{variants.slice(1).map(v => row(v))}</details></div> : row(root);
      })}
      {!project.mcaps.length && <Empty icon={<Upload size={28} />} title="Load your first recording" action={uploadButton}>Drop .mcap recordings together with their translation files, or load recordings from the shared server folder.</Empty>}
      {!!project.mcaps.length && !roots.length && <Empty icon={<Layers3 size={28} />} title="No matching recordings">Try another recording name.</Empty>}
    </section>
    <div className="source-note"><FolderOpen size={16} /><span>Project: <code>{project.path}</code></span></div>
    {server && <ServerRecordings project={project} busy={!!busy} perform={workspace.perform} onImported={workspace.serverImported} onClose={() => setServer(false)} />}
  </>;
}
