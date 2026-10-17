import { useState } from "react";
import { useReviewDecision } from "../../hooks/useReviewDecision";
import type { ReviewController } from "../../hooks/useReviewTasks";
import { useAuth } from "../../state/AuthContext";
import type { Project, Recording } from "../../types/models";
import type { ReviewRevision, ReviewScope, ReviewTask } from "../../types/reviews";
import { can } from "../../utils/permissions";
import { framesFor } from "../../utils/recordings";
import { elapsedSeconds, formatReviewTime, frameAtTime, parseReviewTime, type ReviewPosition } from "../../utils/reviewReferences";
import { RevisionFeedback } from "./RevisionFeedback";

type Draft = { message:string; scope:ReviewScope|null };
export function ReviewActions({project,task,revision,controller,onError,onJump,recording,position,starting=false}: {
  project:Project; task:ReviewTask; revision:ReviewRevision; controller:ReviewController;
  onError:(message:string)=>void; onJump:(channel:string,frame:number)=>void;
  recording?:Recording|null; position:ReviewPosition|null; starting?:boolean;
}) {
  const {user,session}=useAuth();
  const {busy:deciding,act}=useReviewDecision(controller,task,revision,onError);
  const busy=deciding||starting;
  const [kind,setKind]=useState<"general"|"frame"|"timestamp">("general");
  const [scopeIndex,setScopeIndex]=useState(0),[message,setMessage]=useState("");
  const scope=revision.scopes[scopeIndex]||revision.scopes[0];
  const [frameId,setFrameId]=useState(scope.start_frame+1),[time,setTime]=useState("");
  const [drafts,setDrafts]=useState<Draft[]>([]);
  const frames=recording ? framesFor(recording,scope.channel_id) : [];
  const seconds=parseReviewTime(time);
  const referencedFrame=kind==="timestamp" ? (seconds===null ? null : frameAtTime(frames,seconds)) : frameId-1;
  const validReference=kind==="general" || (referencedFrame!==null && Number.isInteger(referencedFrame) && referencedFrame>=scope.start_frame && referencedFrame<=scope.end_frame);
  const usePosition=()=>{
    if(!position)return;
    const index=revision.scopes.findIndex((value)=>value.channel_id===position.channel&&position.frame>=value.start_frame&&position.frame<=value.end_frame);
    if(index<0)return;
    setScopeIndex(index);setKind("frame");setFrameId(position.frame+1);
  };
  const positionAvailable=!!position&&revision.scopes.some((value)=>value.channel_id===position.channel&&position.frame>=value.start_frame&&position.frame<=value.end_frame);
  const open=revision.number===task.current_revision&&["submitted","under_review"].includes(revision.status);
  const independent=!session?.workflow.independent_reviewer||revision.submitted_by_id!==user?.id
    || (!!revision.self_review&&revision.reviewer_id===user?.id);
  const available=!revision.reviewer_id||revision.reviewer_id===user?.id;
  const eligible=open&&independent&&available&&can(project,user,"review.view");
  const commentAccess=can(project,user,"review.comment")||can(project,user,"review.reject");
  const validComment=!!message.trim()&&validReference;
  const comment=():Draft=>{
    const referenceTime=referencedFrame===null ? null : elapsedSeconds(frames,referencedFrame);
    return {message:kind==="timestamp"&&referenceTime!==null ? `@ ${formatReviewTime(referenceTime)} · Frame ${referencedFrame!+1}\n${message.trim()}` : message.trim(),scope:kind!=="general"&&referencedFrame!==null?{channel_id:scope.channel_id,start_frame:referencedFrame,end_frame:referencedFrame}:null};
  };
  const feedback=[...drafts,...(message.trim()?[comment()]:[])];
  const decide=async(action:string)=>{
    if(action!=="approve"&&message.trim()&&!validComment)return;
    if(await act(action,action==="approve"?{}:{comments:feedback})) {setDrafts([]);setMessage("");setKind("general");}
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
        <p className="field-help">Play and inspect the data, then approve the whole revision or submit feedback to return it to Feedback received. Individual frames do not need approval.</p>
        <div className="review-comment-form">
          <label>Reference<select aria-label="Reference" disabled={busy} value={kind} onChange={(event)=>setKind(event.target.value as typeof kind)}><option value="general">Whole revision</option><option value="frame">@ Frame ID</option><option value="timestamp">@ Timestamp</option></select></label>
          <button className="button secondary small" disabled={busy||!positionAvailable} onClick={usePosition}>Reference current frame</button>
          {kind!=="general"&&<div className="button-row task-range">
            {revision.scopes.length>1&&<label>Topic or section<select disabled={busy} value={scopeIndex} onChange={(event)=>{const index=Number(event.target.value),selected=revision.scopes[index];setScopeIndex(index);setFrameId(selected.start_frame+1);setTime("");}}>{revision.scopes.map((value,index)=><option key={index} value={index}>Channel {value.channel_id} · {value.start_frame+1}–{value.end_frame+1}</option>)}</select></label>}
            {kind==="frame"?<label>Frame ID<input disabled={busy} type="number" min={scope.start_frame+1} max={scope.end_frame+1} value={frameId} onChange={(event)=>setFrameId(Number(event.target.value))}/></label>:<label>Timestamp<input aria-label="Timestamp" disabled={busy||!frames.length} value={time} onChange={(event)=>setTime(event.target.value)} placeholder="0:12.500"/><span className="field-help">Elapsed from the start of this topic; seconds or h:mm:ss.mmm.</span></label>}
            <button className="text-button" disabled={!validReference||busy||referencedFrame===null} onClick={()=>referencedFrame!==null&&onJump(scope.channel_id,referencedFrame)}>Jump to reference</button>
            {!validReference&&<p className="field-help" role="status">Enter a reference within the selected submitted section.</p>}
          </div>}
          <label>Feedback<textarea aria-label="Review feedback" disabled={busy} rows={4} maxLength={7800} value={message} onChange={(event)=>setMessage(event.target.value)} placeholder="Describe what needs to change. Optionally reference a timestamp or frame ID above."/></label>
          <button className="button secondary small" disabled={busy||!validComment} onClick={()=>{setDrafts((values)=>[...values,comment()]);setMessage("");}}>Add another feedback item</button>
        </div>
        {drafts.map((draft,index)=><article className="revision-comment" key={index}><div className="button-row"><button className="text-button" disabled={!draft.scope} onClick={()=>draft.scope&&onJump(draft.scope.channel_id,draft.scope.start_frame)}>{draft.scope?draft.scope.start_frame===draft.scope.end_frame?`Frame ${draft.scope.start_frame+1}`:`Frames ${draft.scope.start_frame+1}–${draft.scope.end_frame+1}`:"General comment"}</button>
          <button className="text-button danger" disabled={busy} onClick={()=>setDrafts((values)=>values.filter((_,i)=>i!==index))}>Remove draft</button></div><p>{draft.message}</p></article>)}
      </div>}
      <div className="review-decision-buttons">
        {can(project,user,"review.reject")&&<button className="button secondary" disabled={busy||(!feedback.length&&!revision.comments.some((comment)=>comment.actor_id===user?.id))||(!!message.trim()&&!validComment)} onClick={()=>void decide("request_changes")}>Submit feedback</button>}
        {can(project,user,"review.approve")&&<button className="button primary" disabled={busy||!!drafts.length||!!message.trim()||!revision.snapshot_available} onClick={()=>void decide("approve")}>Approve revision</button>}
        {!can(project,user,"review.reject")&&can(project,user,"review.comment")&&<button className="button secondary" disabled={busy||!feedback.length||(!!message.trim()&&!validComment)} onClick={()=>void decide("comment")}>Publish comments</button>}
      </div>
    </>}
  </div></section>;
}
