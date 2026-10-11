import type { RecordingSummary } from "../types/models";
import type { ReviewStatus, ReviewTask } from "../types/reviews";

// Revision numbers are local to a task. An MCAP's overview follows its newest
// submission, while separate topic/range tasks remain available in history.
export function latestRecordingTask(tasks: ReviewTask[], recordingId: string) {
  return tasks.filter((task) => task.recording_id === recordingId).sort((a, b) => {
    if (!!a.latest_revision !== !!b.latest_revision) return a.latest_revision ? -1 : 1;
    const aDate = a.latest_revision?.submitted_at || a.created_at;
    const bDate = b.latest_revision?.submitted_at || b.created_at;
    return bDate.localeCompare(aDate) || b.id.localeCompare(a.id);
  })[0];
}

export function workflowStatus(task?: ReviewTask, recording?: RecordingSummary): ReviewStatus {
  const revision = task?.latest_revision;
  if (revision) return revision.status === "under_review" && !revision.review_started_at ? "submitted" : revision.status;
  if (task) return task.status;
  return (recording?.summary?.annotated_count || recording?.summary?.reviewed_count || 0) > 0 ? "in_progress" : "unlabeled";
}
