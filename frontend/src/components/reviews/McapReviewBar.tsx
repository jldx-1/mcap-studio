import { useState } from "react";
import type { ReviewController } from "../../hooks/useReviewTasks";
import type { Project, RecordingSummary } from "../../types/models";
import type { ReviewScope, ReviewStatus, ReviewTask } from "../../types/reviews";
import { statusLabel } from "../../types/reviews";
import { workflowStatus } from "../../utils/reviews";
import { ReviewerAssignment } from "./ReviewerAssignment";
import { RevisionFeedback } from "./RevisionFeedback";
import { WorkingCopyAssignment } from "./WorkingCopyAssignment";

export function McapReviewBar({ project, task, recording, controller, annotating, onOpen, onHistory, onComments, onJump }: {
  project: Project; task?: ReviewTask; recording?: RecordingSummary; controller: ReviewController;
  annotating?: boolean;
  onOpen?: () => void; onHistory?: () => void; onComments?: (scope?: ReviewScope) => void;
  onJump?: (channel: string, frame: number) => void;
}) {
  const [commentsOpen, setCommentsOpen] = useState(false);
  const state = !task?.latest_revision && annotating ? "in_progress" : workflowStatus(task, recording);
  const revision = task?.latest_revision;
  const phases: ReviewStatus[] = ["unlabeled", "in_progress", "submitted", "under_review", state === "changes_requested" ? "changes_requested" : "approved"];
  const index = phases.indexOf(state);
  return <section className="mcap-review-bar" aria-label="MCAP review status">
    <div className="workflow-heading"><strong>Review status</strong><span className={`pill workflow-current-status ${state === "approved" ? "green" : state === "changes_requested" ? "amber" : "blue"}`}>{statusLabel[state]}</span></div>
    <ol className="workflow-steps" aria-label="Review workflow">
      {phases.map((phase, i) => <li key={phase} className={i === index ? "is-current" : i < index ? "is-complete" : ""}
        aria-current={i === index ? "step" : undefined}><span>{i + 1}</span>{statusLabel[phase]}</li>)}
    </ol>
    {task ? <>
      <div className="workflow-details">
        <span>Revision: <strong>{task.current_revision || "Working copy"}</strong></span>
        <span>Annotator: <strong>{revision?.submitted_by_name || task.annotator_name}</strong></span>
        <ReviewerAssignment project={project} task={task} controller={controller} />
        <span>Review comments: <strong>{revision?.comments.length || 0}</strong></span>
        {task.previous_revision && <span>Previous revision: <strong>{statusLabel[task.previous_revision.status]}</strong> · {task.previous_revision.comment_count} comments</span>}
      </div>
      <p className="field-help">{task.scopes.map((scope) => `Channel ${scope.channel_id} · Frames ${scope.start_frame + 1}–${scope.end_frame + 1}`).join("; ")}</p>
      {task.working_dirty && task.current_revision > 0 && <p className="field-help">Working-copy changes are waiting for a new submission. This status describes the submitted revision.</p>}
      <WorkingCopyAssignment project={project} task={task} controller={controller} />
      <div className="button-row workflow-buttons">
        {onOpen && task.current_revision > 0 && <button className="button primary small" onClick={onOpen}>Open review</button>}
        {!!revision?.comments.length && <button className="button secondary small" onClick={() => onComments ? onComments() : setCommentsOpen((value) => !value)}>View comments ({revision.comments.length})</button>}
        {onHistory && task.current_revision > 0 && <button className="button secondary small" onClick={onHistory}>Revision history</button>}
        {revision?.comments.filter((comment) => comment.scope).map((comment) => <button className="text-button" key={comment.id}
          disabled={!onJump && !onComments} title={comment.message}
          onClick={() => onJump ? onJump(comment.scope!.channel_id, comment.scope!.start_frame) : onComments?.(comment.scope!)}>
          {comment.scope!.start_frame === comment.scope!.end_frame ? `Frame ${comment.scope!.start_frame + 1}` : `Frames ${comment.scope!.start_frame + 1}–${comment.scope!.end_frame + 1}`}
        </button>)}
      </div>
      {commentsOpen && revision && <RevisionFeedback revision={revision} onJump={onJump} />}
    </> : <p className="field-help">{state === "in_progress" ? "Working-copy annotations have not been submitted yet." : "Open an MCAP to start annotating, then submit a revision for review."}</p>}
  </section>;
}
