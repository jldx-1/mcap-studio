import { useEffect, useMemo, useState } from "react";
import { History, Send } from "lucide-react";
import type { Project, Recording } from "../../types/models";
import type { ReviewRevision } from "../../types/reviews";
import { statusLabel } from "../../types/reviews";
import type { ReviewController } from "../../hooks/useReviewTasks";
import { useAuth } from "../../state/AuthContext";
import { can } from "../../utils/permissions";
import { latestRecordingTask, workflowStatus } from "../../utils/reviews";
import { submissionSummary } from "../../utils/annotationTimeline";
import { api } from "../../services/http";
import Dialog from "../shared/Dialog";
import { RevisionHistory } from "./RevisionHistory";

export function EditorReviewControls({project,recording,channel,controller,save,onJump,onOpen,readOnly}:{project:Project;recording:Recording;channel:string;controller:ReviewController;save:()=>Promise<boolean>;onJump:(channel:string,frame:number)=>void;onOpen:()=>void;readOnly:boolean}) {
  const {user}=useAuth();
  const [dialog,setDialog]=useState<"submit"|"history"|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const [revisions,setRevisions]=useState<Record<string,ReviewRevision[]>>({}),[historyLoading,setHistoryLoading]=useState(false);
  const [taskId,setTaskId]=useState(""),[start,setStart]=useState(1),[end,setEnd]=useState(1);
  const count=recording.index.topics[channel]?.frames.length||0;
  const tasks=controller.tasks.filter(task=>task.recording_id===recording.uuid&&!task.dataset_id&&task.scopes.some(scope=>scope.channel_id===channel));
  const own=tasks.filter(task=>task.annotator_id===user?.id),current=latestRecordingTask(tasks,recording.uuid),selected=own.find(task=>task.id===taskId);
  const ranges=selected?selected.scopes.filter(scope=>scope.channel_id===channel):[{start_frame:start-1,end_frame:end-1}];
  const summary=useMemo(()=>submissionSummary(recording.annotations,channel,count,ranges),[recording.annotations,channel,count,selected,start,end]);
  const status=current?workflowStatus(current):Object.values(recording.annotations.annotations[channel]||{}).some(row=>row.boxes?.length||row.labels?.length||row.reviewed||(row.fields&&JSON.stringify(row.fields)!=="{}"))?"in_progress":"unlabeled";
  const valid=selected?selected.scopes.every(scope=>scope.channel_id===channel):Number.isInteger(start)&&Number.isInteger(end)&&start>=1&&end>=start&&end<=count;
  const open=(value:"submit"|"history")=>{onOpen();setError("");if(value==="submit"){setTaskId(latestRecordingTask(own,recording.uuid)?.id||"");setStart(1);setEnd(count);}setDialog(value);};
  useEffect(()=>{
    if(dialog!=="history")return;
    let active=true;setHistoryLoading(true);setError("");
    void controller.refresh().then(async()=>{
      const values=await api<typeof tasks>(`/projects/${project.uuid}/tasks`);
      return Promise.all(values.filter(task=>task.recording_id===recording.uuid&&!task.dataset_id&&task.scopes.some(scope=>scope.channel_id===channel)&&task.current_revision>0).map(async task=>[task.id,await api<ReviewRevision[]>(`/projects/${project.uuid}/tasks/${task.id}/revisions`)] as const));
    }).then(values=>{if(active)setRevisions(Object.fromEntries(values));}).catch(error=>{if(active)setError(String(error));}).finally(()=>{if(active)setHistoryLoading(false);});
    return()=>{active=false;};
  },[dialog,project.uuid,recording.uuid,channel]);
  return <div className="editor-review-controls">
    {!controller.loading&&<span className="pill workflow-current-status">{statusLabel[status]}</span>}
    <button className="button secondary small" disabled={busy||controller.loading} onClick={()=>open("history")}><History size={15}/>History</button>
    {!readOnly&&can(project,user,"annotation.submit")&&<button className="button primary small" disabled={busy||controller.loading||!count} onClick={()=>open("submit")}><Send size={15}/>Submit for Review</button>}
    {dialog==="submit"&&<Dialog title="Submit for Review" error={error} onClose={()=>{if(!busy)setDialog(null);}}>
      <p className="modal-description">Send this topic’s annotations to the review queue. Your saved work remains editable after submission.</p>
      {!!own.length&&<label>Frame range<select aria-label="Submission range" disabled={busy} value={taskId} onChange={event=>setTaskId(event.target.value)}>{own.map(task=><option key={task.id} value={task.id}>{task.scopes.map(scope=>`Frames ${scope.start_frame+1}–${scope.end_frame+1}`).join(", ")} · revision {task.current_revision+1}</option>)}<option value="">New frame range</option></select></label>}
      {selected?<p className="submission-range">{ranges.map(range=>`Frames ${range.start_frame+1}–${range.end_frame+1}`).join(", ")}</p>:<div className="submission-frame-range"><label>First frame<input aria-label="Task first frame" type="number" min={1} max={count} value={start} disabled={busy} onChange={event=>setStart(Number(event.target.value))}/></label><label>Last frame<input aria-label="Task last frame" type="number" min={start} max={count} value={end} disabled={busy} onChange={event=>setEnd(Number(event.target.value))}/></label></div>}
      <dl className="submission-summary"><div><dt>Frames</dt><dd>{summary.total.toLocaleString()}</dd></div><div><dt>Labeled frames</dt><dd>{summary.labeled.toLocaleString()}</dd></div><div><dt>Boxes</dt><dd>{summary.boxes.toLocaleString()}</dd></div><div><dt>Classes</dt><dd>{summary.classes}</dd></div></dl>
      {!valid&&<p className="inline-error">Choose a valid range within this topic. Use a new frame range for a task covering several topics.</p>}
      <div className="modal-footer"><button className="button secondary" disabled={busy} onClick={()=>setDialog(null)}>Cancel</button><button className="button primary" disabled={busy||!valid} onClick={()=>{setBusy(true);setError("");void(async()=>{
        if(!(await save())){setError("Annotations could not be saved. Close this dialog and retry saving.");return;}
        const task=selected?await controller.current(selected.id):await controller.create({title:recording.source_file.split(/[\\/]/).pop(),recording_id:recording.uuid,scopes:[{channel_id:channel,start_frame:start-1,end_frame:end-1}]});
        await controller.action(task,"submit");setDialog(null);
      })().catch(error=>setError(error instanceof Error?error.message:String(error))).finally(()=>setBusy(false));}}>{busy?"Submitting…":"Confirm submission"}</button></div>
    </Dialog>}
    {dialog==="history"&&<Dialog title="Annotation history" error={error} onClose={()=>setDialog(null)}>
      <div className="editor-history"><button className="button secondary small" disabled={historyLoading} onClick={()=>{setDialog(null);controller.refresh().catch(error=>setError(String(error)));}}>Close history</button>
      {historyLoading?<p role="status">Loading history…</p>:tasks.length?tasks.map(task=><section key={task.id}><h3>{task.title}</h3><RevisionHistory task={task} revisions={revisions[task.id]||[]} onJump={(nextChannel,frame)=>{setDialog(null);onJump(nextChannel,frame);}}/></section>):<p className="muted">No submissions for this topic yet.</p>}</div>
    </Dialog>}
  </div>;
}
