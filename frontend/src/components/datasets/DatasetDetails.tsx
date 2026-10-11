import { ArrowLeft, ArrowRight, BarChart3, Layers3 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, post } from "../../services/http";
import type { Dataset, Project, Recording } from "../../types/models";
import { filename, recordingPath } from "../../utils/recordings";

type Entry = Record<string, unknown>;

interface Point { key: string; mcap_uuid: string; channel_id: string; frame: number; topic: string; x: number; y: number }

interface PCAResult {
  points: Point[]; features: string[]; loadings: number[][]; explained_variance_ratio: number[];
  constant_features: number[]; total_entries: number; sampled_entries: number; candidate_entries: number; unreadable_entries: number; excluded_entries: number; description: string;
}

const entriesFor = (dataset: Dataset) => Array.isArray(dataset.entries)
  ? dataset.entries.map((entry, index) => [String(index), entry] as const)
  : Object.entries(dataset.entries || {});

const textValue = (value: unknown) => value == null ? "" : typeof value === "object" ? JSON.stringify(value) : String(value);

const palette = ["#4b91ff", "#20b78a", "#b078ff", "#ff8b40", "#ec5890", "#14b8cd"];

export default function DatasetDetails({ dataset, project, busy, onOpenRecording }: {
  dataset: Dataset; project: Project; busy: boolean; onOpenRecording: (id: string) => Promise<void>;
}) {
  const entries = useMemo(() => entriesFor(dataset), [dataset]);
  const [source, setSource] = useState("");
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState<string | null>(null);
  const [showPCA, setShowPCA] = useState(false);
  const recordingCache = useRef(new Map<string, Promise<Recording>>());
  const filtered = entries.filter(([, entry]) => !source || `${entry.mcap_uuid}:${entry.channel_id}` === source);
  const entry = entries.find(([key]) => key === selected)?.[1];
  return <div className="dataset-details">
    <div className="collection-body">
      <div className="button-row"><span className="pill blue">{dataset.multimodal ? "Multimodal" : dataset.modalities?.join(" · ") || "No modalities yet"}</span>
        {dataset.multimodal && <span className="muted">{dataset.modalities?.join(" · ")}</span>}
        <button className="button secondary small" onClick={() => setShowPCA((value) => !value)}><BarChart3 size={16} /> {showPCA ? "Hide PCA" : "Explore distribution with PCA"}</button>
      </div>
      <h3 className="dataset-section-title">Included MCAPs and topics</h3>
      <div className="dataset-source-grid">
        {dataset.sources?.map((item) => <div className={`dataset-source-card ${source === `${item.mcap_uuid}:${item.channel_id}` ? "selected" : ""}`} key={`${item.mcap_uuid}:${item.channel_id}`}>
          <button className="row-link" onClick={() => { setSource(`${item.mcap_uuid}:${item.channel_id}`); setPage(0); setSelected(null); }}><strong><Layers3 size={16} /> {filename(item.source_file)}</strong><span>{item.topic} · {item.modality} · {item.entry_count} entries</span></button>
          <button className="text-button" disabled={busy} onClick={() => void onOpenRecording(item.mcap_uuid)}>Open MCAP topic table <ArrowRight size={14} /></button>
        </div>)}
      </div>
      {!!source && <button className="text-button" onClick={() => { setSource(""); setPage(0); }}>Show entries from all MCAPs</button>}
    </div>
    {showPCA && <PCAExplorer key={dataset.uuid} project={project} dataset={dataset} onSelect={setSelected} />}
    {entry && <EntryPreview key={selected} entry={entry} project={project} cache={recordingCache.current} onClose={() => setSelected(null)} />}
    {!entries.length ? <p className="collection-body muted">This dataset has no entries yet.</p> : <>
      <div className="table-wrap"><table>
        <thead><tr><th>MCAP</th><th>Topic</th><th>Frame / message</th>{dataset.columns?.map((column) => <th key={column.name}>{column.name}</th>)}<th>Labels</th></tr></thead>
        <tbody>{filtered.slice(page * 50, (page + 1) * 50).map(([key, value]) => <tr key={key} className={selected === key ? "selected-dataset-entry" : ""}>
          <td>{filename(project.mcaps.find((item) => item.uuid === value.mcap_uuid)?.source_file || String(value.mcap_uuid))}</td>
          <td>{String(value.topic || value.channel_id)}</td><td>{Number(value.frame) + 1}</td>
          {dataset.columns?.map((column) => <td key={column.name}>{textValue((value.values as Entry | undefined)?.[column.name])}</td>)}
          <td><button className="button secondary small" onClick={() => setSelected(key)}>View labels</button></td>
        </tr>)}</tbody>
      </table></div>
      <div className="dataset-selection-footer button-row"><button className="button secondary small" disabled={page === 0} onClick={() => setPage((value) => value - 1)}><ArrowLeft size={15} /> Previous</button>
        <span>{filtered.length ? page * 50 + 1 : 0}–{Math.min((page + 1) * 50, filtered.length)} of {filtered.length} entries</span>
        <button className="button secondary small" disabled={(page + 1) * 50 >= filtered.length} onClick={() => setPage((value) => value + 1)}>Next <ArrowRight size={15} /></button></div>
    </>}
  </div>;
}

function EntryPreview({ entry, project, cache, onClose }: { entry: Entry; project: Project; cache: Map<string, Promise<Recording>>; onClose: () => void }) {
  const [recording, setRecording] = useState<Recording | null>(null);
  const [raw, setRaw] = useState<unknown>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const previewRef = useRef<HTMLElement>(null);
  useEffect(() => { previewRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" }); }, []);
  const id = String(entry.mcap_uuid), channel = String(entry.channel_id), frame = Number(entry.frame);
  useEffect(() => {
    let active = true;
    const base = recordingPath(project.uuid, id);
    if (!cache.has(base)) cache.set(base, api<Recording>(base).catch((error) => { cache.delete(base); throw error; }));
    void cache.get(base)!.then(async (value) => {
      const topic = value.topics.find((item) => String(item.channel_id) === channel);
      const raw = topic?.kind !== "image" && !entry.values ? await api(`${recordingPath(project.uuid, id)}/records/${channel}/${frame}`) : null;
      if (active) { setRecording(value); setRaw(raw); }
    }).catch((error) => { if (active) setError(String(error)); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [project.uuid, id, channel, frame, entry.values, cache]);
  const topic = recording?.topics.find((item) => String(item.channel_id) === channel);
  const row = recording?.annotations.annotations[channel]?.[frame];
  const imageFrame = recording?.index.topics[channel]?.frames[frame];
  return <section ref={previewRef} className="dataset-entry-preview collection-body">
    <div className="button-row"><h3>{String(entry.topic)} · {topic?.kind === "image" ? "Frame" : "Message"} {frame + 1}</h3><button className="text-button" onClick={onClose}>Close preview</button></div>
    {loading && <p role="status">Loading entry labels…</p>}{error && <p className="inline-error" role="alert">{error}</p>}
    {recording && topic?.kind === "image" && <>
      <svg className="dataset-frame-preview" viewBox={`0 0 ${imageFrame?.width || 1} ${imageFrame?.height || 1}`} role="img" aria-label={`Labeled frame ${frame + 1}`}>
        <image href={`/api${recordingPath(project.uuid, id)}/frames/${channel}/${frame}`} width={imageFrame?.width || 1} height={imageFrame?.height || 1} />
        {row?.boxes.map((box, index) => <g key={index}><rect x={box.x} y={box.y} width={box.width} height={box.height} fill="none" stroke={palette[box.class_id % palette.length]} strokeWidth="2" vectorEffect="non-scaling-stroke" /><text x={box.x + 3} y={Math.max(12, box.y - 3)} fill={palette[box.class_id % palette.length]} fontSize={Math.max(12, (imageFrame?.width || 320) / 40)}>{recording.annotations.classes[box.class_id]}{box.track_id ? ` #${box.track_id}` : ""}</text></g>)}
      </svg>
      <p className="field-help">{row?.boxes.length || 0} boxes · {row?.labels?.map((id) => recording.annotations.classes[id]).join(", ") || "No frame classes"}. Labels reflect the annotations currently saved on the source MCAP.</p>
    </>}
    {!!entry.values && <div className="table-wrap"><table><tbody>{Object.entries(entry.values as Entry).map(([name, value]) => <tr key={name}><th>{name}</th><td>{textValue(value)}</td></tr>)}</tbody></table></div>}
    {raw != null && <pre className="dataset-raw-preview">{JSON.stringify(raw, null, 2)}</pre>}
    {recording && topic?.kind !== "image" && !entry.values && <pre className="dataset-raw-preview">{JSON.stringify(row?.fields || row?.labels || {}, null, 2)}</pre>}
  </section>;
}

function PCAExplorer({ dataset, project, onSelect }: { dataset: Dataset; project: Project; onSelect: (key: string) => void }) {
  const [fields, setFields] = useState<string[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [standardize, setStandardize] = useState(true);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [description, setDescription] = useState("");
  const [result, setResult] = useState<PCAResult | null>(null);
  const [colorBy, setColorBy] = useState("mcap");
  const [source, setSource] = useState("");
  const [featureRows, setFeatureRows] = useState<{ mcap_uuid: string; channel_id: string; values: Record<string, number> }[]>([]);
  const base = `/projects/${project.uuid}/datasets/${dataset.uuid}`;
  useEffect(() => {
    let active = true;
    void api<{ fields: string[]; description: string; unreadable_entries: number; rows: { mcap_uuid: string; channel_id: string; values: Record<string, number> }[] }>(`${base}/features`).then((value) => {
      if (active) { setFeatureRows(value.rows); setFields(value.fields); setDescription(value.description); setSelected(value.fields.slice(0, 8)); }
    }).catch((error) => { if (active) setError(String(error)); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [base]);
  const groups = [...new Set(result?.points.map((point) => colorBy === "mcap" ? point.mcap_uuid : `${point.mcap_uuid}:${point.channel_id}`) || [])];
  const groupFor = (point: Point) => colorBy === "mcap" ? point.mcap_uuid : `${point.mcap_uuid}:${point.channel_id}`;
  const bounds = (axis: "x" | "y") => {
    if (!result) return [0, 1];
    let min = Infinity, max = -Infinity;
    for (const point of result.points) { min = Math.min(min, point[axis]); max = Math.max(max, point[axis]); }
    const pad = max - min > 1e-10 ? (max - min) * 0.08 : 1;
    return [min - pad, max + pad];
  };
  const [xMin, xMax] = bounds("x"), [yMin, yMax] = bounds("y");
  return <section className="pca-explorer collection-body">
    <h3>Dataset distribution · PCA</h3><p className="field-help">{description} PCA centers the selected features and projects complete rows onto two components. Up to 5,000 evenly sampled entries are analyzed.</p>
    {loading && <p role="status">Computing features…</p>}
    {error && <p className="inline-error" role="alert">{error}</p>}
    <label>Analyze source<select value={source} disabled={loading} onChange={(event) => {
      const source = event.target.value;
      const available = [...new Set(featureRows.filter((row) => !source || `${row.mcap_uuid}:${row.channel_id}` === source).flatMap((row) => Object.keys(row.values)))].sort();
      setSource(source); setFields(available); setSelected(available.slice(0, 8)); setResult(null);
    }}><option value="">All sources (shared complete features)</option>{dataset.sources?.map((item) => <option key={`${item.mcap_uuid}:${item.channel_id}`} value={`${item.mcap_uuid}:${item.channel_id}`}>{filename(item.source_file)} · {item.topic} · {item.modality}</option>)}</select></label>
    <div className="pca-feature-list">{fields.map((field) => <label className="check-label" key={field}><input type="checkbox" checked={selected.includes(field)} disabled={loading || (!selected.includes(field) && selected.length >= 32)} onChange={(event) => { setResult(null); setSelected((current) => event.target.checked ? [...current, field] : current.filter((name) => name !== field)); }} /><code>{field}</code></label>)}</div>
    <div className="button-row"><label className="check-label"><input type="checkbox" checked={standardize} disabled={loading} onChange={(event) => { setStandardize(event.target.checked); setResult(null); }} /> Standardize feature scales</label>
      <button className="button primary small" disabled={loading || selected.length < 2} onClick={() => { setLoading(true); setError(""); setResult(null); void post<PCAResult>(`${base}/pca`, { features: selected, standardize, source }).then(setResult).catch((error) => setError(String(error))).finally(() => setLoading(false)); }}>Apply PCA ({selected.length} features)</button>
      {result && <label>Color by<select value={colorBy} onChange={(event) => setColorBy(event.target.value)}><option value="mcap">MCAP</option><option value="topic">Topic</option></select></label>}
    </div>
    {!loading && fields.length < 2 && <p className="muted">At least two numeric or boolean features are needed for PCA.</p>}
    {result && <>
      <p className="field-help">{result.points.length} projected / {result.candidate_entries} readable rows in scope / {result.sampled_entries} sampled / {result.total_entries} total entries · {result.excluded_entries} excluded from projection · {result.unreadable_entries} unreadable entries across the sample. Click a point to inspect its labels.{result.constant_features.length > 0 ? ` ${result.constant_features.length} constant features contribute no variance.` : ""}</p>
      <svg className="pca-plot" viewBox="0 0 760 430" role="img" aria-label="PCA dataset distribution">
        <line x1="60" y1="375" x2="735" y2="375" stroke="currentColor" /><line x1="60" y1="25" x2="60" y2="375" stroke="currentColor" />
        <text x="390" y="418" textAnchor="middle" fill="currentColor">PC1 · {(result.explained_variance_ratio[0] * 100).toFixed(1)}% variance</text>
        <text transform="translate(18 200) rotate(-90)" textAnchor="middle" fill="currentColor">PC2 · {(result.explained_variance_ratio[1] * 100).toFixed(1)}% variance</text>
        {[0, 0.25, 0.5, 0.75, 1].map((fraction) => <g key={fraction}><text x={60 + fraction * 675} y="396" textAnchor="middle" fill="currentColor" fontSize="10">{(xMin + fraction * (xMax - xMin)).toFixed(2)}</text><text x="54" y={379 - fraction * 350} textAnchor="end" fill="currentColor" fontSize="10">{(yMin + fraction * (yMax - yMin)).toFixed(2)}</text></g>)}
        {result.points.map((point) => <circle key={point.key} cx={60 + (point.x - xMin) / (xMax - xMin) * 675} cy={375 - (point.y - yMin) / (yMax - yMin) * 350} r="4" opacity="0.7" fill={palette[groups.indexOf(groupFor(point)) % palette.length]} role="button" tabIndex={0} aria-label={`Inspect ${point.topic} frame ${point.frame + 1}`} onClick={() => onSelect(point.key)} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); onSelect(point.key); } }}><title>{filename(project.mcaps.find((item) => item.uuid === point.mcap_uuid)?.source_file || point.mcap_uuid)} · {point.topic} · frame {point.frame + 1}</title></circle>)}
      </svg>
      <div className="pca-legend">{groups.map((group, index) => { const point = result.points.find((point) => groupFor(point) === group)!; return <span key={group}><i style={{ background: palette[index % palette.length] }} />{filename(project.mcaps.find((item) => item.uuid === point.mcap_uuid)?.source_file || point.mcap_uuid)}{colorBy === "topic" ? ` · ${point.topic}` : ""}</span>; })}</div>
      <details><summary>Feature loadings</summary><div className="table-wrap"><table><thead><tr><th>Feature</th><th>PC1</th><th>PC2</th></tr></thead><tbody>{result.features.map((name, index) => <tr key={name}><td>{name}</td><td>{result.loadings[0][index].toFixed(3)}</td><td>{result.loadings[1][index].toFixed(3)}</td></tr>)}</tbody></table></div></details>
    </>}
  </section>;
}
