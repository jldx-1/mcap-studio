import { useEffect, useRef, useState } from "react";
import { api, post } from "../../services/http";
import type { Project } from "../../types/models";
import type { PreparationJob } from "../../types/preparation";
import type { Perform } from "../../types/workspace";
import { filename } from "../../utils/recordings";

export function PreparationJobs({ project, busy, perform, onJobs, onProject, onLabel, canManage }: {
  project: Project; busy: boolean; perform: Perform; onJobs: (jobs: PreparationJob[]) => void;
  onProject: (project: Project) => Promise<void>; onLabel: (id: string) => void; canManage: boolean;
}) {
  const [jobs, setJobs] = useState<PreparationJob[]>([]);
  const [error, setError] = useState("");
  const callbacks = useRef({ onJobs, onProject }); callbacks.current = { onJobs, onProject };
  const seenReady = useRef(new Set<string>());
  useEffect(() => {
    let active = true; let fetching = false;
    const refresh = async () => {
      if (fetching) return; fetching = true;
      try {
        const result = await api<PreparationJob[]>(`/projects/${project.uuid}/workflow/jobs`);
        if (!active) return;
        setJobs(result); setError(""); callbacks.current.onJobs(result);
        const fresh = result.filter(j => j.status === "ready" && !seenReady.current.has(j.uuid));
        if (fresh.length) {
          const updated = await api<Project>(`/projects/${project.uuid}`);
          if (active) { await callbacks.current.onProject(updated); fresh.forEach(j => seenReady.current.add(j.uuid)); }
        }
      } catch (err) { if (active) setError(String(err)); }
      finally { fetching = false; }
    };
    void refresh(); const timer = window.setInterval(() => { if (document.visibilityState === "visible") void refresh(); }, 2500);
    return () => { active = false; clearInterval(timer); };
  }, [project.uuid]);
  const pending = jobs.filter(j => j.status === "queued" || j.status === "running");
  if (!jobs.length && !error) return null;
  return <details className="panel preparation-jobs"><summary>Preparation jobs · {pending.length} in progress · {jobs.filter(j => j.status === "ready").length} ready</summary>{error && <p className="inline-error">{error}</p>}{jobs.slice(0, 20).map(job => <div className="recording-picker-row" key={job.uuid}><div><strong>{filename(job.source_file)}</strong><span className="table-secondary" role={job.status === "running" ? "status" : undefined}>{job.stage}</span>{job.error && <span className="inline-error">{job.error}</span>}</div><span className={`pill ${job.status === "ready" ? "green" : job.status === "failed" ? "amber" : "blue"}`}>{job.status}</span>{job.output_recording && <button className="button secondary small" disabled={busy} onClick={() => onLabel(job.output_recording!)}>Start labeling</button>}{job.status === "failed" && canManage && <button className="button secondary small" disabled={busy} onClick={() => void perform("Retrying preparation", async () => { const retry = await post<PreparationJob>(`/projects/${project.uuid}/workflow/jobs`, job.request); setJobs(current => [retry, ...current]); })}>Retry</button>}</div>)}</details>;
}
