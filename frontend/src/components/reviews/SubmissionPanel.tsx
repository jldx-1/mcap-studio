import { useEffect,useMemo,useState } from "react";
import type { ReviewController } from "../../hooks/useReviewTasks";
import { api } from "../../services/http";
import { useAuth } from "../../state/AuthContext";
import type { Project,Recording } from "../../types/models";
import type { ReviewRevision } from "../../types/reviews";
import { statusLabel } from "../../types/reviews";
import { can } from "../../utils/permissions";
import { RevisionHistory } from "./RevisionHistory";
import { McapReviewBar } from "./McapReviewBar";
import { latestRecordingTask } from "../../utils/reviews";

export function SubmissionPanel({project,recording,channel,controller,save,onError,onJump,datasetId}:{project:Project;recording:Recording;channel:string;controller:ReviewController;save:()=>Promise<boolean>;onError:(message:string)=>void;onJump?:(channel:string,frame:number)=>void;datasetId?:string}){
  const {user}=useAuth();const [busy,setBusy]=useState(false),[revisions,setRevisions]=useState<Record<string,ReviewRevision[]>>({});
  const count=recording.index.topics[channel]?.frames.length||0;
  const [start,setStart]=useState(1),[end,setEnd]=useState(count);
  const tasks=controller.tasks.filter((t)=>t.recording_id===recording.uuid&&(t.dataset_id||"")===(datasetId||"")&&t.scopes.some((s)=>s.channel_id===channel));
  const current = latestRecordingTask(tasks,recording.uuid);
  const [historyOpen,setHistoryOpen] = useState(false);
  const annotating = useMemo(() => Object.values(recording.annotations.annotations).some((rows) =>
    Object.values(rows).some((row) => row.reviewed || row.boxes?.length || row.labels?.length
      || (row.fields != null && JSON.stringify(row.fields) !== "{}"))),[recording.annotations.annotations]);
  const versionKey=tasks.map((t)=>`${t.id}/${t.current_revision}/${t.latest_revision?.version}`).join(",");
  useEffect(()=>{let active=true;void Promise.all(tasks.filter((t)=>t.current_revision>0).map(async(t)=>[t.id,await api<ReviewRevision[]>(`/projects/${project.uuid}/tasks/${t.id}/revisions`)] as const)).then((values)=>{if(active)setRevisions(Object.fromEntries(values));}).catch((err)=>{if(active)onError(String(err));});return()=>{active=false;};},[project.uuid,versionKey,onError]);
  const act=async(run:()=>Promise<unknown>)=>{setBusy(true);try{await run();}catch(err){onError(err instanceof Error?err.message:String(err));await controller.refresh().catch(()=>{});}finally{setBusy(false);}};
  const submit=(id?:string)=>void act(async()=>{
    if(!(await save()))return;
    const task=id?await controller.current(id):await controller.create({title:recording.source_file.split(/[\\/]/).pop(),recording_id:recording.uuid,dataset_id:datasetId,scopes:[{channel_id:channel,start_frame:start-1,end_frame:end-1}]});
    await controller.action(task,"submit");
  });
  return <section className="panel submission-panel"><div className="panel-heading"><div><h2>Working copy and revisions</h2><p>Save freely in the working copy. Every submission publishes a separate, permanent snapshot for review.</p></div><button className="button secondary small" disabled={busy||controller.loading} onClick={()=>void act(controller.refresh)}>Refresh status</button></div><div className="collection-body">
    {!controller.loading&&<McapReviewBar project={project} recording={recording} task={current} controller={controller} annotating={annotating} onJump={onJump} onHistory={()=>setHistoryOpen((value)=>!value)}/>}
    {controller.loading&&<p role="status">Loading revisions…</p>}{tasks.map((task)=><article className="task-card" key={task.id}><div className="button-row"><strong>{task.title}</strong><span className="pill blue">{task.current_revision?`Revision ${task.current_revision}`:"Working copy"}</span><span className="pill">{statusLabel[task.status]}</span>{task.working_dirty&&task.current_revision>0&&<span className="pill amber">Working-copy changes</span>}</div><p>{task.annotator_name} · {task.reviewer_name?`Reviewer: ${task.reviewer_name}`:"Shared review queue"} · {task.scopes.map((s)=>`Frames ${s.start_frame+1}–${s.end_frame+1}`).join(", ")}</p>
      {task.annotator_id===user?.id&&can(project,user,"annotation.submit")&&<button className="button primary small" disabled={busy} onClick={()=>submit(task.id)}>Save and submit revision {task.current_revision+1}</button>}
      {!!revisions[task.id]?.length&&<details className="revision-context" open={historyOpen} onToggle={(event)=>setHistoryOpen(event.currentTarget.open)}><summary>Revision history · {revisions[task.id].length} submissions</summary><RevisionHistory task={task} revisions={revisions[task.id]} onJump={onJump}/></details>}
    </article>)}
    {can(project,user,"annotation.submit")&&<details open={!tasks.length}><summary>{tasks.length?"Submit another frame range":"Submit first revision"}</summary><div className="button-row task-range"><label>First frame<input aria-label="Task first frame" type="number" min={1} max={count} value={start} onChange={(e)=>setStart(Number(e.target.value))}/></label><label>Last frame<input aria-label="Task last frame" type="number" min={start} max={count} value={end} onChange={(e)=>setEnd(Number(e.target.value))}/></label><button className="button primary" disabled={busy||controller.loading||!count||!Number.isInteger(start)||!Number.isInteger(end)||start<1||end<start||end>count} onClick={()=>submit()}>Save and submit revision</button></div></details>}
  </div></section>;
}
