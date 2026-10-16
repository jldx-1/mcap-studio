import { useEffect, useState } from "react";
import { api, post } from "../../services/http";
import type { Project, RecordingSummary } from "../../types/models";
import type { Perform } from "../../types/workspace";
import Dialog from "../shared/Dialog";

interface Listing { root: string; path: string; entries: { name: string; path: string; directory: boolean; bytes: number }[] }
export function ServerRecordings({ project, busy, perform, onImported, onClose }: { project: Project; busy: boolean; perform: Perform; onImported: (recordings: RecordingSummary[]) => Promise<void>; onClose: () => void }) {
  const [path, setPath] = useState("");
  const [listing, setListing] = useState<Listing>();
  const [selected, setSelected] = useState<string[]>([]);
  const [error, setError] = useState("");
  useEffect(() => { let active = true; setError(""); void api<Listing>(`/projects/${project.uuid}/server-recordings?path=${encodeURIComponent(path)}`).then(value => { if (active) setListing(value); }).catch(err => { if (active) setError(String(err)); }); return () => { active = false; }; }, [project.uuid, path]);
  return <Dialog title="Load recordings already on the server" error={error} onClose={() => { if (!busy) onClose(); }}><p>Browse the shared import folder. Selected recordings are copied directly on the server without uploading through your browser.</p><code className="server-import-path">{listing?.root}{path ? `/${path}` : ""}</code>{path && <button className="text-button" disabled={busy} onClick={() => setPath(path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "")}>Parent folder</button>}{listing?.entries.map(entry => <div className="recording-picker-row" key={entry.path}>{entry.directory ? <button className="text-button" disabled={busy} onClick={() => setPath(entry.path)}>{entry.name}/</button> : <label className="check-label"><input type="checkbox" disabled={busy} checked={selected.includes(entry.path)} onChange={e => setSelected(current => e.target.checked ? [...current, entry.path] : current.filter(p => p !== entry.path))} />{entry.name} · {(entry.bytes / 1024 / 1024).toFixed(1)} MB</label>}</div>)}{listing && !listing.entries.length && <p className="field-help">Place MCAP files in this folder on the host, then reopen this browser.</p>}<div className="modal-footer"><button className="button secondary" disabled={busy} onClick={onClose}>Cancel</button><button className="button primary" disabled={busy || !selected.length} onClick={() => void perform("Loading server recordings", async () => {
    const imported: RecordingSummary[] = []; const failures: string[] = [];
    for (const source of selected) { try { imported.push(await post<RecordingSummary>(`/projects/${project.uuid}/server-recordings`, { path: source })); setSelected(current => current.filter(p => p !== source)); } catch (err) { failures.push(`${source}: ${String(err)}`); } }
    if (imported.length) await onImported(imported);
    if (failures.length) setError(failures.join("\n")); else onClose();
  })}>Load {selected.length} recordings</button></div></Dialog>;
}
