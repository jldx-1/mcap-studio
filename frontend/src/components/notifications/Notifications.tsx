import { Bell, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { WorkspaceController } from "../../hooks/useWorkspace";
import { api, post } from "../../services/http";
import type { Project } from "../../types/models";
import type { PreparationJob } from "../../types/preparation";
import type { ReviewTask } from "../../types/reviews";
import { statusLabel } from "../../types/reviews";
import { useAuth } from "../../state/AuthContext";
import { can } from "../../utils/permissions";
import { filename } from "../../utils/recordings";

export function Notifications({workspace,project}: {workspace: WorkspaceController;project: Project}) {
  const {user,session}=useAuth();
  const [open,setOpen]=useState(false),[jobs,setJobs]=useState<PreparationJob[]>([]),[tasks,setTasks]=useState<ReviewTask[]>([]);
  const [error,setError]=useState("");
  const storageKey=`mcap-notifications/${user?.id}/${project.uuid}`;
  const [seen,setSeen]=useState<string[]>(()=>{try{const value:unknown=JSON.parse(localStorage.getItem(storageKey)||"[]");return Array.isArray(value)?value.filter((item):item is string=>typeof item==="string"):[];}catch{return [];}});
  const root=useRef<HTMLDivElement>(null),button=useRef<HTMLButtonElement>(null);
  const callbacks=useRef(workspace);callbacks.current=workspace;
  const ready=useRef(new Set<string>());
  const reviewAccess=can(project,user,"review.view")||can(project,user,"annotation.view");
  const jobAccess=can(project,user,"mcap.view");
  useEffect(()=>{
    let active=true,fetching=false;
    const refresh=async()=>{
      if(fetching)return;fetching=true;
      try {
        const results=await Promise.allSettled([
          jobAccess?api<PreparationJob[]>(`/projects/${project.uuid}/workflow/jobs`):Promise.resolve([]),
          reviewAccess?api<ReviewTask[]>(`/projects/${project.uuid}/tasks`):Promise.resolve([]),
        ]);
        if(!active)return;
        const [jobResult,taskResult]=results;
        setError(results.some(result=>result.status==="rejected")?"Some notifications could not be refreshed. Retrying automatically.":"");
        if(taskResult.status==="fulfilled")setTasks(taskResult.value);
        if(jobResult.status==="fulfilled"){
          const values=jobResult.value;setJobs(values);callbacks.current.resolveLabelingJobs(values);
          const fresh=values.filter(job=>job.status==="ready"&&!ready.current.has(job.uuid));
          if(fresh.length){
            const updated=await api<Project>(`/projects/${project.uuid}`);
            if(active){await callbacks.current.updateProject(updated);fresh.forEach(job=>ready.current.add(job.uuid));}
          }
        }
      } catch {if(active)setError("Notifications could not be refreshed. Retrying automatically.");}
      finally{fetching=false;}
    };
    void refresh();const timer=window.setInterval(()=>{if(document.visibilityState==="visible")void refresh();},5000);
    return()=>{active=false;window.clearInterval(timer);};
  },[project.uuid,jobAccess,reviewAccess]);
  useEffect(()=>{
    if(!open)return;
    const dismiss=(event:PointerEvent)=>{if(!root.current?.contains(event.target as Node))setOpen(false);};
    const escape=(event:KeyboardEvent)=>{if(event.key==="Escape"){setOpen(false);button.current?.focus();}};
    document.addEventListener("pointerdown",dismiss);document.addEventListener("keydown",escape);
    return()=>{document.removeEventListener("pointerdown",dismiss);document.removeEventListener("keydown",escape);};
  },[open]);
  const notifications=tasks.flatMap(task=>{
    const revision=task.latest_revision;if(!revision)return [];
    const reviewer=can(project,user,"review.view")&&(!revision.reviewer_id||revision.reviewer_id===user?.id)
      &&(!session?.workflow.independent_reviewer||revision.submitted_by_id!==user?.id||revision.self_review);
    const review=reviewer&&["submitted","under_review"].includes(revision.status);
    const own=task.annotator_id===user?.id||revision.submitted_by_id===user?.id;
    if(!review&&!(own&&["changes_requested","approved"].includes(revision.status)))return [];
    const name=filename(project.mcaps.find(recording=>recording.uuid===task.recording_id)?.source_file||task.title);
    return [{id:`review/${task.id}/${revision.number}/${revision.status}/${revision.reviewer_id}`,task,name,
      status:statusLabel[revision.status],detail:review?`${revision.submitted_by_name} submitted revision ${revision.number} for review`:`Revision ${revision.number} · ${revision.comments.length} feedback items`}];
  });
  const ids=[...notifications.map(item=>item.id),...jobs.map(job=>`job/${job.uuid}/${job.status}`)];
  const unread=ids.filter(id=>!seen.includes(id)).length;
  const identity=ids.join("|");
  useEffect(()=>{
    if(!open)return;
    const current=identity?identity.split("|"):[];
    setSeen(current);try{localStorage.setItem(storageKey,JSON.stringify(current));}catch{/* Notifications still work without local storage. */}
  },[open,identity,storageKey]);
  const running=jobs.some(job=>job.status==="queued"||job.status==="running");
  return <div className="notifications" ref={root}>
    <button ref={button} className="icon-button notification-toggle" aria-label={unread?`Notifications, ${unread} unread`:"Notifications"} aria-expanded={open} aria-controls="notification-panel" onClick={()=>setOpen(value=>!value)}>
      <Bell size={20}/>{running&&<span className="notification-running" title="Jobs in progress"/>}{unread>0&&<span className="notification-count">{unread>99?"99+":unread}</span>}
    </button>
    {open&&<section className="notification-panel panel" id="notification-panel" aria-label="Notifications">
      <div className="panel-heading"><div><h2>Notifications</h2><p>{notifications.length} reviews · {jobs.filter(job=>["queued","running"].includes(job.status)).length} jobs in progress</p></div><button className="icon-button" aria-label="Close notifications" onClick={()=>{setOpen(false);button.current?.focus();}}><X size={18}/></button></div>
      <div className="notification-list">
        {error&&<p className="inline-error" role="status">{error}</p>}
        {!notifications.length&&!jobs.length&&!error&&<p className="muted">You’re all caught up. Review requests and job updates appear here.</p>}
        {notifications.map(item=><article className="notification-item" key={item.id}><strong>{item.name}</strong><span className={`pill ${item.status==="Approved"?"green":"blue"}`}>{item.status}</span><p>{item.detail}</p><button className="button secondary small" disabled={!!workspace.busy} onClick={()=>{setOpen(false);void workspace.openReview({taskId:item.task.id,view:item.task.latest_revision?.status==="changes_requested"?"comments":"review"});}}>Open {item.task.latest_revision?.status==="changes_requested"?"feedback":"review"}</button></article>)}
        {jobs.map(job=><article className="notification-item" key={job.uuid}><strong>{filename(job.source_file)}</strong><span className={`pill ${job.status==="ready"?"green":job.status==="failed"?"amber":"blue"}`}>{job.status==="ready"?"Ready to label":job.status==="running"?"Processing":job.status==="failed"?"Failed":"Queued"}</span><p>{job.error||job.stage}</p>{job.output_recording&&<button className="button secondary small" disabled={!!workspace.busy} onClick={()=>{setOpen(false);void workspace.startLabeling(job.output_recording!,[...new Set([...workspace.labelingQueue,job.output_recording!])]);}}>Start labeling</button>}{job.status==="failed"&&can(project,user,"project.manage")&&<button className="button secondary small" disabled={!!workspace.busy} onClick={()=>void workspace.perform("Retrying job",async()=>{const retry=await post<PreparationJob>(`/projects/${project.uuid}/workflow/jobs`,job.request);setJobs(values=>[retry,...values]);})}>Retry</button>}</article>)}
      </div>
    </section>}
  </div>;
}
