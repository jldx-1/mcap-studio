import { useState } from "react";
import type { ApprovedSource } from "../../types/datasetVersions";
import type { LibraryRecording, RecordingReference } from "../../types/preparation";
import { filename, formatDuration } from "../../utils/recordings";

export const recordingKey = (r: RecordingReference) => `${r.project_uuid}/${r.recording_id}`;
export function RecordingPicker({ recordings, selected, approved, project, busy, onToggle, onOpen }: {
  recordings: LibraryRecording[]; selected: RecordingReference[]; approved: ApprovedSource[]; project: string; busy: boolean;
  onToggle: (recording: LibraryRecording, checked: boolean) => void; onOpen: (recording: LibraryRecording) => void;
}) {
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const [projectFilter, setProjectFilter] = useState(project);
  const projects = [...new Map(recordings.map(r => [r.project_uuid, r.project_name])).entries()];
  const ready = (r: LibraryRecording) => approved.some(s => s.project_uuid === r.project_uuid && s.recording_id === r.uuid);
  const chosen = (r: LibraryRecording) => selected.some(s => s.project_uuid === r.project_uuid && s.recording_id === r.uuid);
  const rows = recordings.filter(r => (!projectFilter || r.project_uuid === projectFilter) && `${r.source_file} ${r.project_name}`.toLowerCase().includes(search.toLowerCase()) && (filter === "all" || (filter === "ready" ? ready(r) : filter === "selected" ? chosen(r) : !ready(r))));
  return <section className="panel">
    <div className="panel-heading"><div><h2>Choose existing recordings</h2><p>Select recordings now and finish labeling later. Drafts retain recordings that are waiting for approval.</p></div><span className="pill">{selected.length} selected</span></div>
    <div className="collection-body approved-filter-controls"><label>Project<select value={projectFilter} onChange={e => setProjectFilter(e.target.value)}><option value="">All accessible projects</option>{projects.map(([id, name]) => <option key={id} value={id}>{name}</option>)}</select></label><label>Search recordings<input value={search} onChange={e => setSearch(e.target.value)} placeholder="Recording or project name" /></label><label>Show<select value={filter} onChange={e => setFilter(e.target.value)}><option value="all">All recordings</option><option value="ready">Ready to include</option><option value="pending">Needs labeling or review</option><option value="selected">Selected recordings</option></select></label></div>
    <div className="collection-body button-row"><button className="button secondary small" disabled={busy || !rows.length} onClick={() => rows.filter(r => !chosen(r) && (r.project_uuid === project || r.can_export)).forEach(r => onToggle(r, true))}>Select matching recordings</button><span>{rows.length} matching recordings</span></div>
    {rows.map(r => <div className="recording-picker-row" key={`${r.project_uuid}/${r.uuid}`}><label className="check-label"><input type="checkbox" aria-label={`Include recording ${filename(r.source_file)}`} checked={chosen(r)} disabled={busy || (r.project_uuid !== project && !r.can_export)} onChange={e => onToggle(r, e.target.checked)} /><div><strong>{filename(r.source_file)}</strong><span className="table-secondary">{r.project_name}{r.summary ? ` · ${formatDuration(r.summary.duration_seconds)} · ${r.summary.topic_count} topics` : " · Not indexed yet"}{r.source_recording_uuid ? " · Prepared recording" : ""}</span></div></label><span className={`pill ${ready(r) ? "green" : "amber"}`}>{ready(r) ? "Approved data available" : "Needs labeling / review"}</span><button className="button secondary small" disabled={busy} onClick={() => onOpen(r)}>{ready(r) ? "Open recording" : "Continue labeling"}</button></div>)}
    {!rows.length && <div className="empty"><h3>No recordings match</h3><p>Load MCAPs into a project or adjust these filters.</p></div>}
  </section>;
}
