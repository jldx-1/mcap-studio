import { useEffect, useState } from "react";
import { api, post } from "../../services/http";
import type { Project, RecordingSummary, Workflow } from "../../types/models";
import type { PreparationJob, PreparationRecipe, PreparationTopic, TopicRule, TranslationProfile } from "../../types/preparation";
import type { Perform } from "../../types/workspace";
import { filename } from "../../utils/recordings";

const emptySync: Workflow["sync"] = { enabled: false, parent_channel: "", mode: "nearest", tolerance_ns: 100e6, delete_out_of_sync: false };
const matches = (p: TranslationProfile, t: PreparationTopic) => (!p.topic || p.topic === t.topic) && (!p.source_message || p.source_message === t.source_message) && (!p.source_schema_sha256 || p.source_schema_sha256 === t.source_schema_sha256);
const topicMatch = (rule: TopicRule, topic: PreparationTopic) => rule.topic.endsWith("*") ? topic.topic.startsWith(rule.topic.slice(0, -1)) : rule.topic === topic.topic;

export async function readTranslationFiles(files: File[]): Promise<TranslationProfile[]> {
  return Promise.all(files.map(async file => {
    if (file.size > 256 * 1024) throw new Error(`${file.name}: translation files must be smaller than 256 KiB.`);
    const text = await file.text();
    const value = file.name.toLowerCase().endsWith(".json") ? JSON.parse(text) : { script: text };
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${file.name}: expected a translation object or script text.`);
    const script = typeof value.script === "string" ? value.script : Array.isArray(value.mappings) ? value.mappings.map((m: { target: string; expression: string }) => `${m.target} <- ${m.expression}`).join("\n") : null;
    if (script === null) throw new Error(`${file.name}: translation needs script text or mappings.`);
    if (value.output_schema) throw new Error(`${file.name}: Protobuf output descriptors are not supported by native preparation.`);
    return { ...value, name: value.name || file.name, script } as TranslationProfile;
  }));
}

function TranslationPreview({ project, recording, topic, script }: { project: string; recording: string; topic: string; script: string }) {
  const [preview, setPreview] = useState<{ source: unknown; translated: unknown }>();
  const [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      void api<{ source: unknown; translated: unknown }>(`/projects/${project}/recordings/${recording}/translations/preview`, { method: "POST", body: JSON.stringify({ channel_id: topic, script: script || "* -> *" }), signal: controller.signal }).then(value => { setPreview(value); setError(""); }).catch(err => { if (!controller.signal.aborted) setError(String(err)); });
    }, 350);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [project, recording, topic, script]);
  return <div><div className="translation-preview-grid"><div><strong>Recorded input</strong><pre>{preview ? JSON.stringify(preview.source, null, 2) : "Loading sample…"}</pre></div><div><strong>Translated output</strong><pre>{error || (preview ? JSON.stringify(preview.translated, null, 2) : "Updating preview…")}</pre></div></div></div>;
}

export function RecordingPreparation({ project, recordings, translations, busy, perform, onNotice, onQueued, onCancel }: {
  project: Project; recordings: RecordingSummary[]; translations: TranslationProfile[]; busy: boolean; perform: Perform;
  onNotice: (message: string) => void; onQueued: (jobs: PreparationJob[], startLabeling: boolean) => void; onCancel: () => void;
}) {
  const [index, setIndex] = useState(0);
  const [topics, setTopics] = useState<PreparationTopic[]>([]);
  const [rules, setRules] = useState<Record<string, TopicRule>>({});
  const [recipes, setRecipes] = useState<PreparationRecipe[]>([]);
  const [recipe, setRecipe] = useState<PreparationRecipe | null>(null);
  const [name, setName] = useState("");
  const [sync, setSync] = useState(emptySync);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [expanded, setExpanded] = useState("");
  const [jobs, setJobs] = useState<PreparationJob[]>([]);
  const [startLabeling, setStartLabeling] = useState(true);
  const recording = recordings[index];
  const base = `/projects/${project.uuid}`;
  useEffect(() => { let active = true; void api<PreparationRecipe[]>(`${base}/workflow/recipes`).then(items => { if (active) setRecipes(items); }).catch(err => { if (active) setError(String(err)); }); return () => { active = false; }; }, [base]);
  useEffect(() => {
    if (!recording) return;
    let active = true; setLoading(true); setError(""); setExpanded("");
    void api<{ topics: PreparationTopic[] }>(`${base}/recordings/${recording.uuid}/translations`).then(result => {
      if (!active) return;
      setTopics(result.topics);
      setRules(Object.fromEntries(result.topics.map(t => {
        const saved = recipe?.rules.find(r => topicMatch(r, t) && (!r.source_schema_sha256 || r.source_schema_sha256 === t.source_schema_sha256) && (!r.source_message || r.source_message === t.source_message));
        const dropped = translations.filter(p => matches(p, t) && (p.topic || p.source_message || p.source_schema_sha256));
        const presets = t.choices.filter(p => !p.output_schema);
        const automatic = dropped.length === 1 ? dropped[0] : dropped.length > 1 ? null : presets.length === 1 ? presets[0] : null;
        return [t.channel_id, saved ? { ...saved, topic: t.topic, source_message: t.source_message, source_schema_sha256: t.source_schema_sha256 } : { topic: t.topic, source_message: t.source_message, source_schema_sha256: t.source_schema_sha256, keep: true, required: false, script: automatic?.script || "", technique: /image|camera/i.test(`${t.topic} ${t.source_message}`) ? "yolo" : "", label_list_id: "" }];
      })));
      const parent = result.topics.find(t => t.topic === recipe?.sync?.parent_topic)?.channel_id || "";
      setSync(recipe?.sync ? { ...recipe.sync, parent_channel: parent } : emptySync);
    }).catch(err => { if (active) setError(String(err)); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [base, recording?.uuid, recipe, translations]);
  const update = (id: string, patch: Partial<TopicRule>) => setRules(current => ({ ...current, [id]: { ...current[id], ...patch } }));
  const problems = (recipe?.rules || []).filter(r => r.required && (!topics.some(t => topicMatch(r, t)) || topics.some(t => topicMatch(r, t) && ((r.source_schema_sha256 && r.source_schema_sha256 !== t.source_schema_sha256) || (r.source_message && r.source_message !== t.source_message)))));
  const unexpected = recipe ? topics.filter(t => !recipe.rules.some(r => topicMatch(r, t))) : [];
  const schemaChanges = recipe ? topics.filter(t => recipe.rules.some(r => topicMatch(r, t) && ((r.source_schema_sha256 && r.source_schema_sha256 !== t.source_schema_sha256) || (r.source_message && r.source_message !== t.source_message)))) : [];
  const selected = topics.filter(t => rules[t.channel_id]?.keep);
  const makeRequest = (items: PreparationTopic[], mapping: Record<string, TopicRule>, source: RecordingSummary) => ({
    recording_uuid: source.uuid, selected_channels: items.filter(t => mapping[t.channel_id].keep).map(t => t.channel_id),
    translations: Object.fromEntries(items.filter(t => mapping[t.channel_id].keep && mapping[t.channel_id].script.trim()).map(t => [t.channel_id, mapping[t.channel_id].script])),
    techniques: Object.fromEntries(items.filter(t => mapping[t.channel_id].keep && mapping[t.channel_id].technique).map(t => [t.channel_id, mapping[t.channel_id].technique])),
    topic_settings: Object.fromEntries(items.filter(t => mapping[t.channel_id].keep).map(t => [t.channel_id, { technique: mapping[t.channel_id].technique, label_list_id: mapping[t.channel_id].label_list_id }])),
    sync, recipe_name: name || recipe?.name || null, recipe_sha256: recipe?.sha256 || null,
  });
  const queue = (batch: boolean) => void perform("Queueing recording preparation", async () => {
    setError(""); const created: PreparationJob[] = [];
    const inputs = batch ? recordings.slice(index) : [recording];
    for (const source of inputs) {
      try {
        const items = source.uuid === recording.uuid ? topics : (await api<{ topics: PreparationTopic[] }>(`${base}/recordings/${source.uuid}/translations`)).topics;
        const mapping: Record<string, TopicRule> = {};
        for (const item of items) {
          const rule = Object.values(rules).find(r => topicMatch(r, item));
          if (!rule) throw new Error(`Unexpected topic ${item.topic}. Configure this recording individually.`);
          if ((rule.source_schema_sha256 && rule.source_schema_sha256 !== item.source_schema_sha256) || (rule.source_message && rule.source_message !== item.source_message)) throw new Error(`Schema mismatch for ${item.topic}. Inspect this recording individually.`);
          mapping[item.channel_id] = rule;
        }
        const missing = Object.values(rules).filter(r => r.required && !items.some(t => topicMatch(r, t)));
        if (missing.length) throw new Error(`Missing required topics: ${missing.map(r => r.topic).join(", ")}`);
        const request = makeRequest(items, mapping, source);
        if (sync.enabled) {
          const parentTopic = topics.find(t => t.channel_id === sync.parent_channel)?.topic;
          request.sync = { ...sync, parent_channel: items.find(t => t.topic === parentTopic)?.channel_id || "" };
        }
        created.push(await post<PreparationJob>(`${base}/workflow/jobs`, request));
      } catch (err) { setError(`${filename(source.source_file)}: ${String(err)}. Successfully queued recordings remain available.`); break; }
    }
    setJobs(current => [...current, ...created]);
    if (created.length === inputs.length) {
      if (batch || index === recordings.length - 1) onQueued([...jobs, ...created], startLabeling);
      else setIndex(index + 1);
    }
    if (created.length) onNotice(`${created.length} recording${created.length === 1 ? "" : "s"} queued for preparation. You can keep working.`);
  });
  if (!recording) return null;
  return <div className="feature-stack">
    <section className="panel"><div className="panel-heading"><div><h2>Prepare {filename(recording.source_file)}</h2><p>Recording {index + 1} of {recordings.length} · Choose retained topics, translations, and labeling settings.</p></div><button className="button secondary small" disabled={busy} onClick={onCancel}>Back to recordings</button></div>
      <div className="collection-body approved-filter-controls"><label>Saved preparation recipe<select disabled={busy || loading} value={recipe?.uuid || ""} onChange={e => { const next = recipes.find(r => r.uuid === e.target.value) || null; setRecipe(next); setName(next?.name || ""); }}><option value="">Configure this recording</option>{recipes.map(r => <option key={r.uuid} value={r.uuid}>{r.name}</option>)}</select></label><label>Recipe name<input value={name} onChange={e => setName(e.target.value)} placeholder="Robot arm training" disabled={busy} /></label><button className="button secondary" disabled={busy || loading || !name.trim() || !topics.length} onClick={() => void perform("Saving preparation recipe", async () => { const saved = await post<PreparationRecipe>(`${base}/workflow/recipes`, { uuid: recipe?.uuid, revision: recipe?.revision, name: name.trim(), rules: Object.values(rules), sync: { ...sync, parent_topic: topics.find(t => t.channel_id === sync.parent_channel)?.topic } }); setRecipes(current => [saved, ...current.filter(r => r.uuid !== saved.uuid)]); setRecipe(saved); onNotice("Preparation recipe saved for reuse."); })}>Save recipe</button></div>
    </section>
    {error && <p className="inline-error" role="alert">{error}</p>}
    {!!problems.length && <p className="inline-error">Required topics are missing or changed: {problems.map(r => r.topic).join(", ")}. Choose another recipe or configure this recording.</p>}
    {!!schemaChanges.length && <p className="inline-error">Schema changed for {schemaChanges.map(t => t.topic).join(", ")}. Previous scripts were not applied; inspect these topics before continuing.</p>}
    {!!unexpected.length && <p className="field-help">New topics kept for inspection: {unexpected.map(t => t.topic).join(", ")}.</p>}
    {!!translations.length && <p className="field-help">{translations.length} translation files loaded. Exact matches are selected automatically; choose a target for scripts without topic metadata.</p>}
    {loading ? <p role="status">Inspecting recording topics…</p> : <section className="panel"><div className="panel-heading"><div><h2>Retained topics</h2><p>Keeping {selected.length} of {topics.length} topics. {topics.length - selected.length} topics will be removed from the prepared MCAP. The original remains available.</p></div><button className="text-button" disabled={busy} onClick={() => setRules(current => Object.fromEntries(Object.entries(current).map(([id, r]) => [id, { ...r, keep: selected.length !== topics.length }])))}>{selected.length === topics.length ? "Clear all" : "Select all"}</button></div>
      {topics.map(t => { const rule = rules[t.channel_id]; if (!rule) return null; const profiles = [...t.choices.filter(p => !p.output_schema), ...translations.filter(p => matches(p, t))]; return <article className={`preparation-topic ${rule.keep ? "" : "excluded"}`} key={t.channel_id}>
        <div className="approved-filter-controls"><label className="check-label"><input type="checkbox" checked={rule.keep} disabled={busy} onChange={e => update(t.channel_id, { keep: e.target.checked })} /><div><strong>{t.topic}</strong><span className="table-secondary">{t.source_message || t.encoding} · Schema {t.source_schema_sha256.slice(0, 10)}</span></div></label><label>Saved / loaded translation<select aria-label={`Preparation translation for ${t.topic}`} value={profiles.findIndex(p => p.script === rule.script)} disabled={busy || !rule.keep} onChange={e => update(t.channel_id, { script: profiles[Number(e.target.value)]?.script || "" })}><option value={-1}>{rule.script ? "Custom script" : "No translation"}</option>{profiles.map((p, n) => <option value={n} key={`${p.name}/${n}`}>{p.name}</option>)}</select></label><label>Role / labeling format<select disabled={busy || !rule.keep} value={rule.technique} onChange={e => update(t.channel_id, { technique: e.target.value })}><option value="">Associated sensor input</option><option value="yolo">Label bounding boxes</option><option value="classification">Label frame classes</option><option value="csv">Label structured values</option></select></label><label>Label list<select disabled={busy || !rule.keep} value={rule.label_list_id} onChange={e => update(t.channel_id, { label_list_id: e.target.value })}><option value="">All project labels</option>{project.label_lists.map(l => <option value={l.uuid} key={l.uuid}>{l.name}</option>)}</select></label></div>
        <div className="button-row"><label className="check-label"><input type="checkbox" checked={rule.required} disabled={busy} onChange={e => update(t.channel_id, { required: e.target.checked })} />Required in this recipe</label><button className="text-button" disabled={busy || !rule.keep} onClick={() => setExpanded(expanded === t.channel_id ? "" : t.channel_id)}>{expanded === t.channel_id ? "Close script" : "Inspect or edit translation"}</button></div>
        {expanded === t.channel_id && <div className="feature-stack"><label>Translation script<textarea aria-label={`Preparation script for ${t.topic}`} rows={5} disabled={busy} value={rule.script} placeholder="* -> *" onChange={e => update(t.channel_id, { script: e.target.value })} /></label><TranslationPreview project={project.uuid} recording={recording.uuid} topic={t.channel_id} script={rule.script} /></div>}
      </article>; })}
    </section>}
    <section className="panel"><div className="collection-body feature-stack"><label className="check-label"><input type="checkbox" checked={sync.enabled} disabled={busy} onChange={e => setSync({ ...sync, enabled: e.target.checked })} />Synchronize during preparation</label><p className="field-help">Leave off to preserve native sensor samples. Dataset creation can align inputs later.</p>{sync.enabled && <div className="approved-filter-controls"><label>Parent topic<select value={sync.parent_channel} onChange={e => setSync({ ...sync, parent_channel: e.target.value })}><option value="">Choose parent</option>{selected.map(t => <option key={t.channel_id} value={t.channel_id}>{t.topic}</option>)}</select></label><label>Method<select value={sync.mode} onChange={e => setSync({ ...sync, mode: e.target.value })}><option value="nearest">Nearest timestamp</option><option value="linear">Linear interpolation</option><option value="rate">Rate alignment</option></select></label><label>Maximum gap (ms)<input type="number" min={0} value={(sync.tolerance_ns || 0) / 1e6} onChange={e => setSync({ ...sync, tolerance_ns: Number(e.target.value) * 1e6 })} /></label></div>}<label className="check-label"><input type="checkbox" checked={startLabeling} onChange={e => setStartLabeling(e.target.checked)} />Start a labeling queue when preparation finishes</label><div className="button-row"><button className="button primary" disabled={busy || loading || !selected.length || !!problems.length || (sync.enabled && !selected.some(t => t.channel_id === sync.parent_channel))} onClick={() => queue(false)}>Prepare this recording</button>{recordings.length - index > 1 && <button className="button secondary" disabled={busy || loading || !selected.length || !!problems.length} onClick={() => queue(true)}>Apply these settings to remaining recordings</button>}<button className="button secondary" disabled={busy} onClick={() => onQueued(jobs, false)}>Prepare in background / return to recordings</button></div></div></section>
  </div>;
}
