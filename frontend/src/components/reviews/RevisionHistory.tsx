import type { ReviewRevision, ReviewTask } from "../../types/reviews";
import { statusLabel } from "../../types/reviews";
import { RevisionFeedback } from "./RevisionFeedback";

export function RevisionHistory({ task, revisions, selected, onSelect, onJump }: {
  task: ReviewTask; revisions: ReviewRevision[]; selected?: number;
  onSelect?: (number: number) => void; onJump?: (channel: string, frame: number) => void;
}) {
  return <div className="revision-history-list" aria-label="Revision history">
    {revisions.map((revision) => <article key={revision.id} className="revision-history-item">
      <div className="button-row"><strong>Revision {revision.number}</strong>
        <span className="pill">{statusLabel[revision.status]}</span><span>{revision.comments.length} comments</span>
        {revision.number === task.current_revision && <span className="muted">Latest submission</span>}
        {onSelect && <button className="button secondary small" aria-pressed={selected === revision.number} onClick={() => onSelect(revision.number)}>Open revision {revision.number}</button>}
      </div>
      <p className="field-help">Submitted by {revision.submitted_by_name} · {new Date(revision.submitted_at).toLocaleString()}<br />
        Reviewer: {revision.reviewer_name || "Shared queue"}{revision.reviewed_at && ` · Reviewed ${new Date(revision.reviewed_at).toLocaleString()}`}</p>
      {!!revision.comments.length && <details><summary>{revision.comments.length} review comments</summary><RevisionFeedback revision={revision} onJump={onJump} /></details>}
    </article>)}
  </div>;
}
