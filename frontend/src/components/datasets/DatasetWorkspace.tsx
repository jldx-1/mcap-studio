import { useEffect, useRef, useState } from "react";
import type { DatasetWorkspaceProps } from "../../hooks/useDatasetWorkspace";
import { sourceKey, useApprovedData } from "../../hooks/useApprovedData";
import { api, post } from "../../services/http";
import { useAuth } from "../../state/AuthContext";
import type { ApprovedSource, DatasetVersion } from "../../types/datasetVersions";
import type { Dataset } from "../../types/models";
import type { DatasetDraft, LibraryRecording, RecordingReference } from "../../types/preparation";
import { revisionKey, selectRevisions, taskKey } from "../../utils/datasetSelection";
import { can } from "../../utils/permissions";
import { ApprovedDataBrowser } from "./ApprovedDataBrowser";
import { RecordingPicker, recordingKey } from "./RecordingPicker";
import { DatasetCreation } from "./DatasetCreation";
import DatasetDetails from "./DatasetDetails";
import { DatasetVersionDetails } from "./DatasetVersionDetails";
import { YoloImport } from "./YoloImport";
import { YoloDatasetDetails } from "./YoloDatasetDetails";

export function DatasetWorkspace({ project, mode, onModeChange, onOpenRecording, onContinueLabeling, busy, perform, onProject, onNotice, initialRecordings = [], onInitialRecordingsUsed }: DatasetWorkspaceProps) {
  const { user } = useAuth();
  const [error, setError] = useState("");
  const data = useApprovedData(project.uuid, setError);
  const [tab, setTab] = useState<"available" | "datasets" | "import">(mode === "new" ? "available" : "datasets");
  const [selected, setSelected] = useState<string[]>([]);
  const [recordings, setRecordings] = useState<LibraryRecording[]>([]);
  const [references, setReferences] = useState<RecordingReference[]>([]);
  const [drafts, setDrafts] = useState<DatasetDraft[]>([]);
  const [draft, setDraft] = useState<DatasetDraft | null>(null);
  const [draftName, setDraftName] = useState("");
  const [draftSelection, setDraftSelection] = useState<DatasetVersion["selection"]>();
  const [target, setTarget] = useState<Dataset | null>(null);
  const [baseVersion, setBaseVersion] = useState<DatasetVersion | null>(null);
  const [dataset, setDataset] = useState<Dataset | null>(null);
  const [version, setVersion] = useState<DatasetVersion | null>(null);
  const [search, setSearch] = useState("");
  const [formKey, setFormKey] = useState(0);
  const [loading, setLoading] = useState(true);
  const [showApproved, setShowApproved] = useState(false);
  const excludedTasks = useRef(new Set<string>());
  const resolvedRecordings = useRef(new Set<string>());
  const base = `/projects/${project.uuid}`;
  const generate = can(project, user, "datasets.generate");
  const refresh = async () => {
    const [library, saved] = await Promise.all([api<LibraryRecording[]>("/recording-library"), api<DatasetDraft[]>(`${base}/workflow/drafts`)]);
    setRecordings(library); setDrafts(saved);
  };
  useEffect(() => { let active = true; void Promise.all([api<LibraryRecording[]>("/recording-library"), api<DatasetDraft[]>(`${base}/workflow/drafts`)]).then(([library, saved]) => { if (active) { setRecordings(library); setDrafts(saved); } }).catch(err => { if (active) setError(String(err)); }).finally(() => { if (active) setLoading(false); }); return () => { active = false; }; }, [base]);
  useEffect(() => {
    if (mode === "new" || mode === "add") setTab("available");
    else if (mode === "load") { setTab("datasets"); setDataset(null); setVersion(null); }
  }, [mode]);
  useEffect(() => {
    if (!initialRecordings.length) return;
    setReferences(current => [...new Map([...current, ...initialRecordings.map(id => ({ project_uuid: project.uuid, recording_id: id }))].map(r => [recordingKey(r), r])).values()]);
    setTab("available"); onModeChange("new"); onInitialRecordingsUsed?.();
  }, [initialRecordings, project.uuid, onInitialRecordingsUsed, onModeChange]);
  // Resolve newly approved tasks without silently replacing an already pinned revision.
  useEffect(() => {
    const latest = new Map<string, ApprovedSource>();
    for (const source of data.catalog.sources) {
      if (!references.some(r => r.project_uuid === source.project_uuid && r.recording_id === source.recording_id && !resolvedRecordings.current.has(recordingKey(r)))) continue;
      const key = taskKey(source);
      if (!latest.has(key) || latest.get(key)!.revision < source.revision) latest.set(key, source);
    }
    setSelected(current => {
      const tasks = new Set(current.map(key => key.slice(0, key.lastIndexOf("/"))));
      const candidates = [...latest.values()];
      const additions = candidates.filter(s => !tasks.has(taskKey(s)) && !excludedTasks.current.has(taskKey(s))).map(sourceKey);
      candidates.forEach(s => resolvedRecordings.current.add(`${s.project_uuid}/${s.recording_id}`));
      return additions.length ? [...current, ...additions] : current;
    });
  }, [data.catalog.sources, references]);
  const ready = data.catalog.sources.filter(s => selected.includes(sourceKey(s)));
  const unresolved = selected.filter(key => !data.catalog.sources.some(s => sourceKey(s) === key));
  const pending = references.filter(r => !ready.some(s => s.project_uuid === r.project_uuid && s.recording_id === r.recording_id));
  const saveDraft = async (selection = draftSelection) => {
    const saved = await post<DatasetDraft>(`${base}/workflow/drafts`, { uuid: draft?.uuid, revision: draft?.revision, name: draftName.trim() || target?.name || "Untitled dataset", recordings: references, selected_sources: selected, target_dataset: target?.uuid || null, base_version: baseVersion, selection });
    setDraft(saved); setDraftName(saved.name); setDrafts(current => [saved, ...current.filter(d => d.uuid !== saved.uuid)]); return saved;
  };
  const choose = (keys: string[]) => {
    for (const key of selected.filter(key => !keys.includes(key))) excludedTasks.current.add(key.slice(0, key.lastIndexOf("/")));
    for (const key of keys) excludedTasks.current.delete(key.slice(0, key.lastIndexOf("/")));
    const next = selectRevisions(selected, keys, data.catalog.sources);
    setSelected(next);
    const added = data.catalog.sources.filter(s => next.includes(sourceKey(s)));
    added.forEach(s => resolvedRecordings.current.add(`${s.project_uuid}/${s.recording_id}`));
    setReferences(current => [...new Map([...current, ...added.map(s => ({ project_uuid: s.project_uuid, recording_id: s.recording_id }))].map(r => [recordingKey(r), r])).values()]);
  };
  const toggleRecording = (r: LibraryRecording, checked: boolean) => {
    if (checked) for (const s of data.catalog.sources.filter(s => s.project_uuid === r.project_uuid && s.recording_id === r.uuid)) excludedTasks.current.delete(taskKey(s));
    const ref = { project_uuid: r.project_uuid, recording_id: r.uuid };
    resolvedRecordings.current.delete(recordingKey(ref));
    setReferences(current => checked ? [...new Map([...current, ref].map(v => [recordingKey(v), v])).values()] : current.filter(v => recordingKey(v) !== recordingKey(ref)));
    if (!checked) setSelected(current => current.filter(key => !data.catalog.sources.some(s => sourceKey(s) === key && s.project_uuid === r.project_uuid && s.recording_id === r.uuid)));
  };
  const startNew = () => { resolvedRecordings.current.clear(); excludedTasks.current.clear(); setTarget(null); setBaseVersion(null); setDraft(null); setDraftName(""); setDraftSelection(undefined); setSelected([]); setReferences([]); setFormKey(k => k + 1); setTab("available"); onModeChange("new"); };
  const loadTarget = async (id: string, preserve: boolean) => {
    const item = project.datasets.find(d => d.uuid === id);
    if (!item) { setTarget(null); setBaseVersion(null); setFormKey(k => k + 1); return; }
    const latest = await api<DatasetVersion>(`${base}/datasets/${id}/versions/${item.latest_version}`);
    latest.source_revisions.forEach(s => resolvedRecordings.current.add(`${s.project_uuid}/${s.recording_id}`));
    setTarget(item); setBaseVersion(latest); setDraftSelection(latest.selection);
    setSelected(current => [...new Set([...latest.source_revisions.map(revisionKey), ...(preserve ? current.filter(key => !latest.source_revisions.some(s => taskKey(s) === key.slice(0, key.lastIndexOf("/")))) : [])])]);
    setReferences(current => [...new Map([...(preserve ? current : []), ...latest.source_revisions.map(s => ({ project_uuid: s.project_uuid, recording_id: s.recording_id }))].map(r => [recordingKey(r), r])).values()]);
    setFormKey(k => k + 1); setTab("available"); onModeChange("add");
  };
  const openDraft = (saved: DatasetDraft) => {
    resolvedRecordings.current.clear(); excludedTasks.current.clear();
    for (const key of saved.selected_sources) { const source=data.catalog.sources.find(s => sourceKey(s)===key); if(source) resolvedRecordings.current.add(`${source.project_uuid}/${source.recording_id}`); }
    setDraft(saved); setDraftName(saved.name); setReferences(saved.recordings); setSelected(saved.selected_sources); setTarget(project.datasets.find(d => d.uuid === saved.target_dataset) || null); setBaseVersion(saved.base_version || null); setDraftSelection(saved.selection); setFormKey(k => k + 1); setTab("available"); onModeChange(saved.target_dataset ? "add" : "new");
  };
  const open = (item: Dataset) => void perform("Opening dataset", async () => {
    const value = item.kind === "approved-dataset" ? item : await api<Dataset>(`${base}/datasets/${item.uuid}`);
    const next = item.kind === "approved-dataset" ? await api<DatasetVersion>(`${base}/datasets/${item.uuid}/versions/${item.latest_version}`) : null;
    setDataset(value); setVersion(next); setTab("datasets"); onModeChange("view");
  });
  const openRecording = async (r: LibraryRecording) => { const ok = !generate || await perform("Saving dataset draft", async () => { await saveDraft(); }); if (ok) await (onContinueLabeling || onOpenRecording)(r.uuid, r.project_uuid); };
  const filtered = project.datasets.filter(item => item.name.toLowerCase().includes(search.toLowerCase()));
  const groups = [
    { title: "Approved dataset versions", items: filtered.filter(item => item.kind === "approved-dataset") },
    { title: "Imported YOLO datasets", items: filtered.filter(item => item.kind === "imported-yolo") },
    { title: "Working label tables", items: filtered.filter(item => !["approved-dataset", "imported-yolo"].includes(item.kind || "")) },
  ];
  return <div className="feature-stack">
    {error && <p className="inline-error" role="alert">{error}</p>}
    <div className="dataset-section-tabs" role="group" aria-label="Dataset sections">
      <button className={`button ${tab === "datasets" ? "primary" : "secondary"}`} disabled={busy} onClick={() => { setTab("datasets"); setDataset(null); setVersion(null); onModeChange("load"); }}>My datasets <span>{project.datasets.length}</span></button>
      <button className={`button ${tab === "available" ? "primary" : "secondary"}`} disabled={busy} onClick={() => { setTab("available"); onModeChange(target ? "add" : "new"); }}>Choose recordings</button>
      {generate && <button className={`button ${tab === "import" ? "primary" : "secondary"}`} disabled={busy} onClick={() => { setTab("import"); onModeChange("view"); }}>Import YOLO</button>}
    </div>
    {tab === "import" && generate && <YoloImport project={project} busy={busy} perform={perform} onProject={onProject} onImported={item => { setDataset(item); setVersion(null); setTab("datasets"); onModeChange("view"); onNotice(`${item.name} imported with ${item.entry_count} frames.`); }} />}
    {tab === "available" && <>
      <section className="panel"><div className="panel-heading"><div><h2>{target ? `Add to ${target.name}` : "Plan a dataset"}</h2><p>{references.length} recordings selected · {ready.length} approved revisions · {pending.length} awaiting labels or review</p></div><button className="button secondary small" disabled={busy} onClick={() => void perform("Refreshing recordings", async () => { await refresh(); await data.refresh(); })}>Refresh recordings</button></div>
        <div className="collection-body approved-filter-controls"><label>Dataset destination<select aria-label="Dataset destination" disabled={busy} value={target?.uuid || ""} onChange={e => { if (e.target.value) void perform("Loading dataset members", () => loadTarget(e.target.value, true)); else { setTarget(null); setBaseVersion(null); setFormKey(k => k + 1); onModeChange("new"); } }}><option value="">Create a new dataset</option>{project.datasets.filter(d => d.kind === "approved-dataset").map(d => <option value={d.uuid} key={d.uuid}>{d.name}</option>)}</select></label><label>Draft name<input value={draftName} onChange={e => setDraftName(e.target.value)} placeholder={target?.name || "Robot training"} /></label>{generate && <button className="button secondary" disabled={busy || loading} onClick={() => void perform("Saving dataset draft", async () => { await saveDraft(); onNotice("Dataset selection saved. Continue labeling whenever you are ready."); })}>Save draft</button>}</div>
      </section>
      {loading ? <p role="status">Loading existing recordings…</p> : <RecordingPicker recordings={recordings} selected={references} approved={data.catalog.sources} project={project.uuid} busy={busy} onToggle={toggleRecording} onOpen={r => void openRecording(r)} />}
      <details open={showApproved} onToggle={e => setShowApproved(e.currentTarget.open)}><summary>Choose exact approved revisions and filter labels or modalities</summary><ApprovedDataBrowser data={data} selected={selected} onSelected={choose} busy={busy} /></details>
      {unresolved.length > 0 && <p className="inline-error">{unresolved.length} selected approved revisions are unavailable. Reload or remove them before publishing; existing members will not be silently dropped.</p>}
      {selected.length > 0 && <section className="panel"><div className="panel-heading"><div><h2>Selected revisions</h2><p>Pinned revisions remain selected when filters change.</p></div></div>{ready.map(s => { const newer = data.catalog.sources.find(v => taskKey(v) === taskKey(s) && v.revision > s.revision); return <div className="recording-picker-row" key={sourceKey(s)}><span>{s.source_file} · Revision {s.revision}</span>{newer && <button className="button secondary small" disabled={busy} onClick={() => choose([...selected, sourceKey(newer)])}>Use approved revision {newer.revision}</button>}<button className="text-button" disabled={busy} onClick={() => { setSelected(current => current.filter(k => k !== sourceKey(s))); setReferences(current => current.filter(r => r.project_uuid !== s.project_uuid || r.recording_id !== s.recording_id)); }}>Remove</button></div>; })}{unresolved.map(key => <div className="recording-picker-row" key={key}><span>Unavailable revision {key}</span><button className="text-button" disabled={busy} onClick={() => setSelected(current => current.filter(k => k !== key))}>Remove unavailable revision</button></div>)}</section>}
      {pending.length > 0 && <p className="field-help">Pending recordings remain in the draft. Publishing includes only the approved revisions listed above.</p>}
      {generate && ready.length > 0 && !unresolved.length && <DatasetCreation key={`${formKey}/${target?.uuid || "new"}`} project={project} target={target} baseVersion={baseVersion} initialName={draftName} initialSelection={draftSelection} sources={ready} data={data} busy={busy} perform={perform} onProject={onProject} onSelectionChange={selection => void perform("Saving draft settings", async () => { setDraftSelection(selection); await saveDraft(selection); onNotice("Draft settings saved."); })} onCreated={(item, next) => { setDataset(item); setVersion(next); setTarget(null); setBaseVersion(null); setTab("datasets"); onModeChange("view"); onNotice(`${item.name} version ${next.number} published with ${next.sample_count.toLocaleString()} samples; ${next.dropped_unsynchronized} samples excluded by synchronization.`); }} />}
    </>}
    {tab === "datasets" && (!dataset ? <>
      <section className="panel"><div className="panel-heading"><div><h2>My datasets</h2><p>Saved training versions, dataset drafts, and imported labels.</p></div>{generate && <button className="button primary" disabled={busy} onClick={startNew}>Create dataset</button>}</div><div className="collection-body"><input aria-label="Search datasets" placeholder="Search dataset names" value={search} onChange={e => setSearch(e.target.value)} /></div></section>
      {!!drafts.length && <section className="panel"><div className="panel-heading"><h2>Dataset drafts</h2></div>{drafts.filter(d => d.name.toLowerCase().includes(search.toLowerCase())).map(d => <div className="recording-picker-row" key={d.uuid}><button className="text-button" disabled={busy} onClick={() => openDraft(d)}>{d.name} · {d.recordings.length} recordings · Continue draft</button>{generate && <button className="text-button danger-text" disabled={busy} onClick={() => void perform("Removing draft", async () => { await api(`${base}/workflow/drafts/${d.uuid}`, { method: "DELETE" }); setDrafts(current => current.filter(item => item.uuid !== d.uuid)); })}>Remove draft</button>}</div>)}</section>}
      {groups.filter(g => g.items.length).map(g => <section className="panel" key={g.title}><div className="panel-heading"><h2>{g.title}</h2></div>{g.items.map(item => <button key={item.uuid} className="recording-row" disabled={busy} onClick={() => open(item)}><div><strong>{item.name}</strong><span>{item.kind === "approved-dataset" ? `Version ${item.latest_version} · ${item.entry_count.toLocaleString()} samples` : `${item.entry_count.toLocaleString()} rows`}</span></div><span className="pill">Open dataset</span></button>)}</section>)}
      {!filtered.length && !drafts.length && <section className="panel"><div className="empty"><h3>No datasets yet</h3><p>Choose existing recordings to plan a dataset, or import YOLO labels.</p></div></section>}
    </> : <>
      <div className="button-row"><button className="button secondary small" disabled={busy} onClick={() => { setDataset(null); setVersion(null); onModeChange("load"); }}>All datasets</button></div>
      {dataset.kind === "imported-yolo" ? <YoloDatasetDetails key={dataset.uuid} project={project} dataset={dataset} busy={busy} onError={setError} /> : version ? <DatasetVersionDetails project={project} dataset={project.datasets.find(item => item.uuid === dataset.uuid) || dataset} version={version} onVersion={setVersion} busy={busy} perform={perform} onNotice={onNotice} onNewVersion={() => void perform("Loading existing dataset members", async () => { setDraft(null); setDraftName(dataset.name); await loadTarget(dataset.uuid, false); })} /> : <section className="panel"><div className="panel-heading"><h2>{dataset.name}</h2></div><DatasetDetails dataset={dataset} project={project} busy={busy} onOpenRecording={id => onOpenRecording(id)} /></section>}
    </>)}
  </div>;
}
