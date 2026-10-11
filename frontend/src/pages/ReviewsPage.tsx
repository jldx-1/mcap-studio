import { useState } from "react";
import { useReviewTasks } from "../hooks/useReviewTasks";
import { useAuth } from "../state/AuthContext";
import type { Project } from "../types/models";
import type { ReviewNavigationTarget } from "../types/reviews";
import { statusLabel } from "../types/reviews";
import type { Perform } from "../types/workspace";
import { filename } from "../utils/recordings";
import { workflowStatus } from "../utils/reviews";

export function ReviewsPage({ project, busy, perform, onError, onOpen }: {
  project: Project; busy: boolean; perform: Perform; onError: (message: string) => void;
  onOpen: (target: ReviewNavigationTarget) => void;
}) {
  const { user } = useAuth();
  const controller = useReviewTasks(project.uuid, onError);
  const [filter, setFilter] = useState("open");
  const [search, setSearch] = useState("");
  const [mine, setMine] = useState(false);
  const names = new Map(project.mcaps.map((recording) => [recording.uuid, filename(recording.source_file)]));
  const submitted = controller.tasks.filter((task) => task.current_revision > 0);
  const taskCounts = new Map<string, number>();
  for (const task of submitted) taskCounts.set(task.recording_id,(taskCounts.get(task.recording_id)||0)+1);
  const tasks = submitted.filter((task) => {
    const status = workflowStatus(task);
    return (filter === "all" || (filter === "open" ? ["submitted", "under_review", "changes_requested"].includes(status) : status === filter))
      && (!mine || task.latest_revision?.reviewer_id === user?.id)
      && (names.get(task.recording_id) || task.title).toLocaleLowerCase().includes(search.trim().toLocaleLowerCase());
  }).sort((a, b) => (b.latest_revision?.submitted_at || "").localeCompare(a.latest_revision?.submitted_at || ""));

  return <div className="feature-stack reviews-queue-page">
    <div className="page-heading"><div><h1>Reviews</h1><p>Open a submitted MCAP revision to approve it or request changes.</p></div>
      <button className="button secondary" disabled={busy} onClick={() => void perform("Refreshing reviews", controller.refresh)}>Refresh</button></div>
    <section className="panel"><div className="panel-heading"><div><h2>{filter === "open" ? "Open reviews" : filter === "approved" ? "Approved reviews" : "Submitted reviews"}</h2><p>{tasks.length} reviews</p></div></div>
      <div className="collection-body review-queue-filters">
        <label>MCAP<input type="search" aria-label="Search reviews" placeholder="Search MCAPs…" value={search} onChange={(event) => setSearch(event.target.value)} /></label>
        <label>Status<select aria-label="Review queue filter" value={filter} onChange={(event) => setFilter(event.target.value)}>
          <option value="open">All open reviews</option><option value="submitted">Ready for review</option><option value="under_review">Under review</option>
          <option value="changes_requested">Changes requested</option><option value="approved">Approved</option><option value="all">All submissions</option>
        </select></label>
        <label className="check-label"><input type="checkbox" checked={mine} onChange={(event) => setMine(event.target.checked)} />Assigned to me</label>
      </div>
      <div className="table-wrap"><table className="review-queue-table"><thead><tr><th>MCAP</th><th>Revision</th><th>Status</th><th>Annotator</th><th>Reviewer</th><th>Comments</th><th>Submitted</th><th /></tr></thead><tbody>
        {tasks.map((task) => <tr key={task.id}>
          <td><strong>{names.get(task.recording_id) || task.title}</strong>{(taskCounts.get(task.recording_id)||0) > 1 && <small>{task.scopes.map((scope) => `Channel ${scope.channel_id} · Frames ${scope.start_frame + 1}–${scope.end_frame + 1}`).join("; ")}</small>}</td>
          <td>{task.current_revision}</td><td><span className={`pill ${workflowStatus(task) === "approved" ? "green" : workflowStatus(task) === "changes_requested" ? "amber" : "blue"}`}>{statusLabel[workflowStatus(task)]}</span></td>
          <td>{task.latest_revision?.submitted_by_name || task.annotator_name}</td><td>{task.latest_revision?.reviewer_name || "Shared queue"}</td>
          <td>{task.latest_revision?.comments.length || 0}</td><td>{task.latest_revision && <time dateTime={task.latest_revision.submitted_at}>{new Date(task.latest_revision.submitted_at).toLocaleString()}</time>}</td>
          <td><button className="button primary small" disabled={busy} onClick={() => onOpen({ taskId: task.id, view: "review" })}>Open review</button></td>
        </tr>)}
      </tbody></table></div>
      {!tasks.length && <p className="collection-body" role="status">{controller.loading ? "Loading reviews…" : "No reviews match these filters."}</p>}
    </section>
  </div>;
}
