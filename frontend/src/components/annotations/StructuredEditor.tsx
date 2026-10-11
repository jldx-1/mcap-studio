import { ArrowLeft, ArrowRight, Database, Plus, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useReviewTasks } from "../../hooks/useReviewTasks";
import { useAuth } from "../../state/AuthContext";
import { can } from "../../utils/permissions";
import { SubmissionPanel } from "../reviews/SubmissionPanel";
import { api, post } from "../../services/http";
import type { Dataset, Project, Recording } from "../../types/models";
import type { Perform } from "../../types/workspace";
import { recordingPath } from "../../utils/recordings";

type Cell = string | number | boolean | null | object;

interface Column {
  id: string;
  name: string;
  source: string | null;
  transform: "identity" | "text" | "number" | "boolean" | "scale";
  scale: number;
  offset: number;
  default: string;
  label_list?: string;
}

interface Preview {
  fields: string[];
  rows: { index: number; time_ns: string; values: Record<string, Cell> }[];
  total: number;
}

const cellText = (value: Cell | undefined) => value == null ? "" : typeof value === "object" ? JSON.stringify(value) : String(value);

function displayCell(value: Cell | undefined, column: Column) {
  if (value == null || value === "") return "";
  if (column.transform === "number" || column.transform === "scale") {
    const number = Number(value);
    if (!Number.isFinite(number)) return "Invalid number";
    return String(column.transform === "scale" ? number * column.scale + column.offset : number);
  }
  if (column.transform === "boolean") {
    const text = cellText(value).trim().toLowerCase();
    return ["true", "1", "yes"].includes(text) ? "true" : ["false", "0", "no"].includes(text) ? "false" : "Invalid boolean";
  }
  return cellText(value);
}

export default function StructuredEditor({
  project,
  recording,
  channel,
  perform,
  busy,
  onProject,
  onNotice,
  onError,
  onBack,
  registerNavigationGuard
}: {
  project: Project; recording: Recording; channel: string; perform: Perform; busy: boolean;
  onProject: (project: Project) => Promise<void>; onNotice: (notice: string) => void; onError: (error: string) => void;
  onBack: () => void; registerNavigationGuard: (guard: (() => Promise<boolean>) | null) => void;
}) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [offset, setOffset] = useState(0);
  const [columns, setColumns] = useState<Column[]>([]);
  const [overrides, setOverrides] = useState<Record<string, Record<string, string>>>({});
  const [name, setName] = useState("");
  const [datasetId, setDatasetId] = useState("");
  const [dirty, setDirty] = useState(false);
  const reviews = useReviewTasks(project.uuid,onError);
  const { user } = useAuth();
  const readOnly = !can(project,user,"datasets.generate") || reviews.loading;
  const dirtyRef = useRef(false);
  dirtyRef.current = dirty;
  const base = `${recordingPath(project.uuid, recording.uuid)}/structured/${channel}`;
  const topic = recording.topics.find((item) => String(item.channel_id) === channel);
  useEffect(() => {
    let active = true;
    setLoading(true);
    setLoadError("");
    void post<Preview>(`${base}/preview`, { offset, limit: 50 }).then((value) => {
      if (active) setPreview(value);
    }).catch((error) => { if (active) { setLoadError(String(error)); onError(String(error)); } }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [base, offset, onError]);
  useEffect(() => {
    registerNavigationGuard(async () => !dirtyRef.current || window.confirm("Discard unsaved column and truth-value changes?"));
    const warn = (event: BeforeUnloadEvent) => { if (dirtyRef.current) event.preventDefault(); };
    window.addEventListener("beforeunload", warn);
    return () => { registerNavigationGuard(null); window.removeEventListener("beforeunload", warn); };
  }, [registerNavigationGuard]);
  const updateColumn = (id: string, changes: Partial<Column>) => {
    setColumns((current) => current.map((column) => column.id === id ? { ...column, ...changes } : column)); setDirty(true);
  };
  const addColumn = (source: string | null) => {
    setColumns((current) => [...current, { id: crypto.randomUUID(), name: source || `label_${current.filter((column) => !column.source).length + 1}`, source, transform: "identity", scale: 1, offset: 0, default: "" }]); setDirty(true);
  };
  const validNames = columns.every((column) => column.name.trim()) && new Set(columns.map((column) => column.name.trim())).size === columns.length;
  const saveDataset = () => perform("Saving CSV dataset",async () => {
        const updated = await post<Project>(`${base}/datasets`, { name: name.trim(), columns, overrides, ...(datasetId ? { dataset_uuid: datasetId } : {}) }); await onProject(updated); setDatasetId(updated.datasets.find((item) => item.name === name.trim())?.uuid || ""); setDirty(false); dirtyRef.current = false; onNotice("Structured dataset saved. Open Datasets to inspect or download its CSV.");
      
  });
  return <section className="panel structured-editor">
    <div className="panel-heading"><div><h2>Message columns</h2><p>{topic?.topic} · {preview?.total.toLocaleString() || 0} messages · one row per message</p></div><button className="button secondary small" disabled={busy} onClick={onBack}><ArrowLeft size={16} /> Back to topic table</button></div>
    <div className="collection-body">
      <label>Dataset<select aria-label="Structured dataset to edit" value={datasetId} disabled={busy} onChange={(event) => {
        const id = event.target.value;
        void (async () => {
          if (dirtyRef.current && !window.confirm("Discard unsaved changes and switch datasets?")) return;
          if (!id) { setDatasetId(""); setColumns([]); setOverrides({}); setName(""); setDirty(false); return; }
          await perform("Loading CSV dataset", async () => {
            const value = await api<Dataset>(`/projects/${project.uuid}/datasets/${id}`);
            setColumns(value.columns as Column[] || []); setOverrides(value.overrides || {}); setName(value.name); setDatasetId(id); setDirty(false);
          });
        })();
      }}><option value="">New CSV dataset</option>{project.datasets.filter((item) => item.format === "csv" && item.source_recording_uuid === recording.uuid && item.source_channel === channel).map((item) => <option key={item.uuid} value={item.uuid}>{item.name}</option>)}</select></label>
    </div>
    <div className="structured-layout">
      <aside className="structured-fields"><h3>Message fields</h3><p className="field-help">Nested objects and repeated fields are flattened into paths. Select the fields to include.</p>
        {loading && !preview && <p role="status">Reading messages…</p>}
        {loadError && <p role="alert" className="inline-error">{loadError}</p>}
        {preview?.fields.map((field) => <label className="check-label" key={field}><input type="checkbox" disabled={busy || readOnly || (columns.length >= 256 && !columns.some((column) => column.source === field))} checked={columns.some((column) => column.source === field)} onChange={(event) => {
          if (event.target.checked) addColumn(field); else { setColumns((current) => current.filter((column) => column.source !== field)); setDirty(true); }
        }} /><code>{field}</code></label>)}
      </aside>
      <div className="structured-column-settings"><div className="button-row"><h3>Selected columns</h3><button className="button secondary small" disabled={busy || columns.length >= 256} onClick={() => addColumn(null)}><Plus size={16} /> Add custom column</button></div>
        {!columns.length && <p className="muted">Select message fields or add a custom truth, class, state, or quality column.</p>}
        {columns.map((column) => <div className="column-config" key={column.id}>
          <label>Column name<input aria-label={`Name for ${column.source || column.name}`} value={column.name} disabled={busy || readOnly} onChange={(event) => updateColumn(column.id, { name: event.target.value })} /></label>
          <label>Conversion<select aria-label={`Conversion for ${column.name}`} value={column.transform} disabled={busy || readOnly} onChange={(event) => updateColumn(column.id, { transform: event.target.value as Column["transform"] })}><option value="identity">Keep original value</option><option value="text">Text</option><option value="number">Number</option><option value="boolean">Boolean truth value</option><option value="scale">Scale and offset</option></select></label>
          {column.transform === "scale" && <><label>Multiply by<input type="number" step="any" value={column.scale} disabled={busy || readOnly} onChange={(event) => updateColumn(column.id, { scale: Number(event.target.value) })} /></label><label>Add offset<input type="number" step="any" value={column.offset} disabled={busy || readOnly} onChange={(event) => updateColumn(column.id, { offset: Number(event.target.value) })} /></label></>}
          {!column.source && <><label>Default value<input value={column.default} disabled={busy || readOnly} onChange={(event) => updateColumn(column.id, { default: event.target.value })} /></label><label>Label list (optional)<select value={column.label_list || ""} disabled={busy || readOnly} onChange={(event) => updateColumn(column.id, { label_list: event.target.value })}><option value="">Free text</option>{project.label_lists.map((list) => <option key={list.uuid} value={list.uuid}>{list.name}</option>)}</select></label></>}
          <button className="icon-button" aria-label={`Remove ${column.name} column`} disabled={busy || readOnly} onClick={() => { setColumns((current) => current.filter((item) => item.id !== column.id)); setOverrides((current) => Object.fromEntries(Object.entries(current).map(([row, values]) => [row, Object.fromEntries(Object.entries(values).filter(([id]) => id !== column.id))]))); setDirty(true); }}><Trash2 size={16} /></button>
          <small>{column.source || "Custom value · edit each message in the table below"}</small>
        </div>)}
        {!validNames && columns.length > 0 && <p role="alert" className="inline-error">Give each column a unique, nonempty name.</p>}
      </div>
    </div>
    {!!columns.length && <div className="table-wrap structured-table"><table><thead><tr><th>Message</th>{columns.map((column) => <th key={column.id}>{column.name}</th>)}</tr></thead><tbody>{preview?.rows.map((row) => <tr key={row.index}><td>{row.index + 1}</td>{columns.map((column) => <td key={column.id}>{column.source ? displayCell(row.values[column.source], column) : (() => {
      const value = overrides[row.index]?.[column.id] ?? column.default;
      const labels = project.label_lists.find((list) => list.uuid === column.label_list)?.label_ids.map((id) => project.labels.find((label) => label.uuid === id)?.name || "");
      const setValue = (text: string) => { setOverrides((current) => ({ ...current, [row.index]: { ...current[row.index], [column.id]: text } })); setDirty(true); };
      return labels ? <select aria-label={`${column.name} for message ${row.index + 1}`} disabled={busy || readOnly} value={value} onChange={(event) => setValue(event.target.value)}><option value="">Unassigned</option>{value && !labels.includes(value) && <option value={value}>{value}</option>}{labels.map((label) => <option key={label} value={label}>{label}</option>)}</select> : <input aria-label={`${column.name} for message ${row.index + 1}`} disabled={busy || readOnly} value={value} onChange={(event) => setValue(event.target.value)} />;
    })()}</td>)}</tr>)}</tbody></table></div>}
    <div className="dataset-selection-footer"><div className="button-row"><button className="button secondary small" disabled={busy || loading || offset === 0} onClick={() => setOffset((value) => Math.max(0, value - 50))}><ArrowLeft size={15} /> Previous</button><span>{preview?.total ? `${offset + 1}–${Math.min(offset + 50, preview.total)} of ${preview.total}` : "No rows"}</span><button className="button secondary small" disabled={busy || loading || !preview || offset + 50 >= preview.total} onClick={() => setOffset((value) => value + 50)}>Next <ArrowRight size={15} /></button></div>
      <label>Dataset name<input value={name} disabled={busy || readOnly} onChange={(event) => { setName(event.target.value); setDirty(true); }} placeholder="e.g. Motor quality labels" /></label><button className="button primary" disabled={busy || readOnly || loading || !!loadError || !preview?.total || !columns.length || !validNames || !name.trim()} onClick={() => void saveDataset()}><Database size={16} /> Save CSV dataset</button>
    </div>
    {datasetId && <SubmissionPanel project={project} recording={recording} channel={channel} datasetId={datasetId} controller={reviews} save={() => dirty ? saveDataset() : Promise.resolve(true)} onError={onError} />}
  </section>;
}
