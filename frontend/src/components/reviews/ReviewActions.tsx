import { useState } from "react";
import { useReviewDecision } from "../../hooks/useReviewDecision";
import type { ReviewController } from "../../hooks/useReviewTasks";
import { useAuth } from "../../state/AuthContext";
import type { Project } from "../../types/models";
import type { ReviewRevision, ReviewScope, ReviewTask } from "../../types/reviews";
import { can } from "../../utils/permissions";
import { RevisionFeedback } from "./RevisionFeedback";

type Draft = { message:string; scope:ReviewScope|null };
export function ReviewActions({project,task,revision,controller,onError,onJump,starting=false}: {
  project:Project; task:ReviewTask; revision:ReviewRevision; controller:ReviewController;
  onError:(message:string)=>void; onJump:(channel:string,frame:number)=>void;
  starting?:boolean;
}) {
  const {user,session}=useAuth();
  const {busy:deciding,act}=useReviewDecision(controller,task,revision,onError);
  const busy=deciding||starting;
  const [kind,setKind]=useState<"range"|"general"|null>(null);
  const [scopeIndex,setScopeIndex]=useState(0),[message,setMessage]=useState("");
  const scope=revision.scopes[scopeIndex]||revision.scopes[0];
  const [start,setStart]=useState(scope.start_frame+1),[end,setEnd]=useState(scope.start_frame+1);
  const [drafts,setDrafts]=useState<Draft[]>([]);
  const open=revision.number===task.current_revision&&["submitted","under_review"].includes(revision.status);
  const independent=!session?.workflow.independent_reviewer||revision.submitted_by_id!==user?.id
    || (!!revision.self_review&&revision.reviewer_id===user?.id);
  const available=!revision.reviewer_id||revision.reviewer_id===user?.id;
  const eligible=open&&independent&&available&&can(project,user,"review.view");
  const commentAccess=can(project,user,"review.comment")||can(project,user,"review.reject");
  const validRange=Number.isInteger(start)&&Number.isInteger(end)&&start>=scope.start_frame+1&&end>=start&&end<=scope.end_frame+1;
  const validComment=!!message.trim()&&(kind!=="range"||validRange);
  const comment=():Draft=>({message:message.trim(),scope:kind==="range"?{channel_id:scope.channel_id,start_frame:start-1,end_frame:end-1}:null});
  const feedback=[...drafts,...(kind&&message.trim()?[comment()]:[])];
  const decide=async(action:string)=>{
    if(await act(action,action==="approve"?{}:{comments:feedback})) {setDrafts([]);setMessage("");setKind(null);}
  };

  return <section className="panel review-actions"><div className="panel-heading"><h2>Review</h2>
  </div><div className="collection-body">
    {!open&&<p className="field-help">This revision is read-only. Its annotations, decision, and feedback are preserved.</p>}
    {open&&!independent&&can(project,user,"review.view")&&<p className="field-help">Choose yourself as the reviewer to review this revision.</p>}
    {open&&!can(project,user,"review.view")&&<p className="field-help">Waiting for a reviewer.</p>}
    {open&&independent&&!available&&<p className="field-help">This revision is assigned to {revision.reviewer_name}. You can inspect its annotations and feedback.</p>}
    {!!revision.comments.length&&<RevisionFeedback revision={revision} onJump={onJump}/>}
    {eligible&&<>
      {commentAccess&&<div className="task-feedback">
        <div className="button-row"><button className="button secondary small" disabled={busy||kind!==null} onClick={()=>{setKind("range");setMessage("");}}>Add frame range comment</button>
          <button className="button secondary small" disabled={busy||kind!==null} onClick={()=>{setKind("general");setMessage("");}}>Add general comment</button></div>
        {kind&&<div className="review-comment-form">
          {kind==="range"&&<div className="button-row task-range">
            {revision.scopes.length>1&&<label>Topic or section<select disabled={busy} value={scopeIndex} onChange={(event)=>{const index=Number(event.target.value),selected=revision.scopes[index];setScopeIndex(index);setStart(selected.start_frame+1);setEnd(selected.start_frame+1);}}>{revision.scopes.map((value,index)=><option key={index} value={index}>Channel {value.channel_id} · {value.start_frame+1}–{value.end_frame+1}</option>)}</select></label>}
            <label>First frame<input disabled={busy} type="number" min={scope.start_frame+1} max={scope.end_frame+1} value={start} onChange={(event)=>setStart(Number(event.target.value))}/></label>
            <label>Last frame<input disabled={busy} type="number" min={start} max={scope.end_frame+1} value={end} onChange={(event)=>setEnd(Number(event.target.value))}/></label>
            <button className="text-button" disabled={!validRange||busy} onClick={()=>onJump(scope.channel_id,start-1)}>Jump to section</button>
          </div>}
          <label>{kind==="range"?"Frame range feedback":"General feedback"}<textarea aria-label="Review feedback" disabled={busy} rows={3} maxLength={8000} value={message} onChange={(event)=>setMessage(event.target.value)} placeholder="Describe the changes needed in this revision."/></label>
          <div className="button-row"><button className="button secondary small" disabled={busy||!validComment} onClick={()=>{setDrafts((values)=>[...values,comment()]);setMessage("");setKind(null);}}>Add comment</button>
            <button className="text-button" disabled={busy} onClick={()=>{setKind(null);setMessage("");}}>Cancel comment</button></div>
        </div>}
        {drafts.map((draft,index)=><article className="revision-comment" key={index}><div className="button-row"><button className="text-button" disabled={!draft.scope} onClick={()=>draft.scope&&onJump(draft.scope.channel_id,draft.scope.start_frame)}>{draft.scope?draft.scope.start_frame===draft.scope.end_frame?`Frame ${draft.scope.start_frame+1}`:`Frames ${draft.scope.start_frame+1}–${draft.scope.end_frame+1}`:"General comment"}</button>
          <button className="text-button danger" disabled={busy} onClick={()=>setDrafts((values)=>values.filter((_,i)=>i!==index))}>Remove draft</button></div><p>{draft.message}</p></article>)}
      </div>}
      <div className="review-decision-buttons">
        {can(project,user,"review.reject")&&<button className="button secondary" disabled={busy||(!feedback.length&&!revision.comments.some((comment)=>comment.actor_id===user?.id))||(!!message.trim()&&!validComment)} onClick={()=>void decide("request_changes")}>Request Changes</button>}
        {can(project,user,"review.approve")&&<button className="button primary" disabled={busy||!!drafts.length||!!message.trim()||!revision.snapshot_available} onClick={()=>void decide("approve")}>Approve Revision</button>}
        {!can(project,user,"review.reject")&&can(project,user,"review.comment")&&<button className="button secondary" disabled={busy||!feedback.length||(!!message.trim()&&!validComment)} onClick={()=>void decide("comment")}>Publish comments</button>}
      </div>
    </>}
  </div></section>;
}
