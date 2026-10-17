import { ArrowLeft, History } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ReviewActions } from "../components/reviews/ReviewActions";
import { ReviewPlayer } from "../components/reviews/ReviewPlayer";
import { ReviewerAssignment } from "../components/reviews/ReviewerAssignment";
import { WorkingCopyAssignment } from "../components/reviews/WorkingCopyAssignment";
import { RevisionComparison } from "../components/reviews/RevisionComparison";
import { RevisionFeedback } from "../components/reviews/RevisionFeedback";
import { RevisionHistory } from "../components/reviews/RevisionHistory";
import { useReviewRevision } from "../hooks/useReviewRevision";
import { useReviewTasks } from "../hooks/useReviewTasks";
import { useAuth } from "../state/AuthContext";
import { can } from "../utils/permissions";
import type { Project } from "../types/models";
import type { ReviewNavigationTarget } from "../types/reviews";
import { statusLabel } from "../types/reviews";
import type { Perform } from "../types/workspace";
import type { ReviewPosition } from "../utils/reviewReferences";
import { filename } from "../utils/recordings";

export function ReviewDetailPage({project,target,busy,perform,onError,onBack}: {
  project:Project; target:ReviewNavigationTarget|null; busy:boolean; perform:Perform;
  onError:(message:string)=>void; onBack:()=>void;
}) {
  const controller = useReviewTasks(project.uuid,onError);
  const {user,session}=useAuth();
  const [position,setPosition]=useState<ReviewPosition|null>(null);
  const [starting,setStarting]=useState(false);
  const task = controller.tasks.find((value)=>value.id===target?.taskId);
  const {revisions,revision,recording,number,setNumber} = useReviewRevision(project.uuid,task,onError);
  const [historyOpen,setHistoryOpen] = useState(target?.view==="history");
  const [jump,setJump] = useState<{channel:string;frame:number;key:number}|null>(null);
  const playerRef = useRef<HTMLDivElement>(null),historyRef = useRef<HTMLElement>(null),feedbackRef = useRef<HTMLDivElement>(null);
  const appliedTarget = useRef(false);
  useEffect(()=>{window.scrollTo({top:0});},[]);
  useEffect(()=>{
    setJump(null);setPosition(null);
  },[number]);
  useEffect(()=>{
    if(appliedTarget.current||!revision||!recording)return;
    appliedTarget.current=true;
    if(target?.view==="history")historyRef.current?.scrollIntoView({block:"start"});
    else if(target?.view==="comments"){
      if(target.scope){setJump({channel:target.scope.channel_id,frame:target.scope.start_frame,key:Date.now()});playerRef.current?.scrollIntoView({block:"start"});}
      else feedbackRef.current?.scrollIntoView({block:"start"});
    }
  },[target,!!revision,!!recording]);
  const navigate=(channel:string,frame:number)=>{setJump({channel,frame,key:Date.now()});playerRef.current?.scrollIntoView({block:"start",behavior:"smooth"});};
  const name=project.mcaps.find((value)=>value.uuid===task?.recording_id)?.source_file;
  const previousFeedback=revisions.find((value)=>value.number<number&&value.comments.length>0);
  const beginReview=async()=>{
    if(starting||!task||!revision||number!==task.current_revision||revision.review_started_at||!["submitted","under_review"].includes(revision.status)||!can(project,user,"review.view"))return;
    if(revision.reviewer_id&&revision.reviewer_id!==user?.id)return;
    if(session?.workflow.independent_reviewer&&revision.submitted_by_id===user?.id&&!revision.self_review)return;
    setStarting(true);
    try{await controller.action({...task,latest_revision:revision},"claim");}
    catch(err){onError(String(err));await controller.refresh().catch(()=>{});}
    finally{setStarting(false);}
  };

  return <div className="feature-stack review-detail-page">
    <div className="review-detail-navigation"><button className="button secondary small" disabled={busy} onClick={onBack}><ArrowLeft size={15}/>Back to reviews</button>
      <button className="button secondary small" disabled={busy} onClick={()=>void perform("Refreshing review",controller.refresh)}>Refresh</button></div>
    {!task||!revision?<section className="panel collection-body"><p role="status">{controller.loading||task?"Loading submitted revision…":"This review is unavailable. Choose a submission from Reviews."}</p></section>:<>
      <section className="panel"><div className="panel-heading"><div><h1>{name?filename(name):task.title}</h1><p>Revision {number} · Submitted by {revision.submitted_by_name} · {new Date(revision.submitted_at).toLocaleString()}</p></div><span className={`pill workflow-current-status ${revision.status==="approved"?"green":revision.status==="changes_requested"?"amber":"blue"}`}>{statusLabel[revision.status]}</span></div>
        <div className="collection-body review-detail-summary">
          {number===task.current_revision?<ReviewerAssignment project={project} task={task} controller={controller}/>:<div className="button-row"><strong>Previous revision · read-only snapshot</strong><span>Reviewer: {revision.reviewer_name||"Shared queue"}</span><button className="button primary small" onClick={()=>setNumber(task.current_revision)}>Open current revision {task.current_revision}</button></div>}
          <button className="button secondary small review-history-toggle" aria-label={historyOpen?"Hide revision history":"Show revision history"} aria-expanded={historyOpen} onClick={()=>setHistoryOpen((value)=>!value)}><History size={14}/>History{revision.comments.length>0&&` · ${revision.comments.length} comments`}</button>
        </div>
      </section>
      <div className="review-workspace" ref={playerRef}>
        {recording?<ReviewPlayer key={`${task.id}/${number}`} project={project} task={{...task,scopes:revision.scopes}} revision={revision} recording={recording} jump={jump} onError={onError} onBeginReview={()=>void beginReview()} onPositionChange={setPosition}/>:<section className="panel collection-body"><p role="status">{revision.snapshot_available?"Loading submitted snapshot…":"This older review has no recoverable annotation snapshot. Its feedback and decision are preserved."}</p></section>}
        <div className="review-feedback-column" ref={feedbackRef}>
          <ReviewActions key={`${task.id}/${number}`} project={project} task={task} revision={revision} controller={controller} onError={onError} onJump={navigate} starting={starting} recording={recording} position={position}/>
        </div>
      </div>
      {previousFeedback&&<section className="panel previous-review-feedback"><div className="panel-heading"><h2>Previous review feedback</h2><span className="pill">Revision {previousFeedback.number}</span></div><div className="collection-body"><RevisionFeedback revision={previousFeedback} onJump={navigate}/><button className="button secondary small" onClick={()=>setNumber(previousFeedback.number)}>Open revision {previousFeedback.number} snapshot</button></div></section>}
      <section className="panel" ref={historyRef} hidden={!historyOpen}><div className="panel-heading"><h2>Revision history</h2></div><div className="collection-body"><RevisionHistory task={task} revisions={revisions} selected={number} onSelect={setNumber} onJump={navigate}/><details className="review-assignment-options"><summary>Working-copy assignment</summary><WorkingCopyAssignment project={project} task={task} controller={controller}/></details></div></section>
      {historyOpen&&<RevisionComparison projectId={project.uuid} taskId={task.id} current={revision} revisions={revisions} onJump={navigate} onError={onError}/>}
    </>}
  </div>;
}
