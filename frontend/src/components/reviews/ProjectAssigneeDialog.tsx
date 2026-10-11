import { useEffect, useRef, useState } from "react";
import { api } from "../../services/http";
import type { Account } from "../../types/accounts";
import Dialog from "../shared/Dialog";

export function ProjectAssigneeDialog({ projectId, role, currentId, excludedId, selfId, selfOnly, allowSharedQueue = true, description, onAssign, onClose }: {
  projectId: string; role: "reviewer" | "annotator"; currentId: string | null;
  excludedId?: string; selfId?: string; selfOnly?: boolean; allowSharedQueue?: boolean; description: string;
  onAssign: (id: string) => Promise<unknown>; onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [directory, setDirectory] = useState<Account[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const search = useRef<HTMLInputElement>(null);
  const permission = role === "reviewer" ? "review.view" : "annotation.submit";
  const plural = role === "reviewer" ? "reviewers" : "annotators";

  useEffect(() => {
    let active = true;
    setLoading(true); setError(""); setDirectory([]);
    search.current?.focus();
    void api<Account[]>(`/projects/${projectId}/directory`).then((values) => {
      if (active) setDirectory(values);
    }).catch((err) => { if (active) setError(String(err)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [projectId]);

  const eligible = directory.filter((account) => account.permissions.includes(permission) && account.id !== excludedId && (!selfOnly || account.id===selfId));
  const normalize = (value: string) => value.normalize("NFKD").toLocaleLowerCase().replace(/\p{M}/gu, "").replace(/[^\p{L}\p{N}]/gu, "");
  const terms = query.trim().split(/\s+/).map(normalize).filter(Boolean);
  const matches = eligible.filter((account) => terms.every((term) => normalize(`${account.name} ${account.username || ""}`).includes(term)))
    .sort((a, b) => Number(b.id===selfId)-Number(a.id===selfId) || a.name.localeCompare(b.name));
  const assign = async (id: string) => {
    setSaving(true); setError("");
    try { await onAssign(id); onClose(); }
    catch (err) { setError(err instanceof Error ? err.message : String(err)); }
    finally { setSaving(false); }
  };

  return <Dialog title={role === "reviewer" ? "Assign reviewer" : "Change annotator"} error={error} onClose={() => { if (!saving) onClose(); }}>
    <div className="assignee-selector">
      <label>Search by name<input ref={search} type="search" aria-label={`Search ${plural}`} placeholder="Type a name…"
        value={query} disabled={saving} onChange={(event) => setQuery(event.target.value)} /></label>
      <p className="field-help">{description}</p>
      <div className="assignee-results" role="group" aria-label={`Eligible ${plural}`} aria-busy={loading || saving}>
        {loading ? <p role="status">Loading {plural}…</p> : <>
          {matches.map((account) => <button key={account.id} type="button" disabled={saving || account.id === currentId}
            aria-pressed={account.id === currentId} onClick={() => void assign(account.id)}>
            <strong>{account.name}{account.id===selfId&&<small> (You)</small>}</strong><span>{account.id === currentId ? "Assigned" : "Assign"}</span>
          </button>)}
          {!matches.length && !error && <p role="status">{eligible.length ? `No ${plural} match your search.` : `No eligible ${plural} in this project.`}</p>}
        </>}
      </div>
      <div className="button-row">
        {role === "reviewer" && allowSharedQueue && <button className="button secondary small" disabled={saving || loading || !currentId} onClick={() => void assign("")}>Use shared review queue</button>}
        <button className="button secondary small" disabled={saving} onClick={onClose}>Cancel</button>
      </div>
    </div>
  </Dialog>;
}
