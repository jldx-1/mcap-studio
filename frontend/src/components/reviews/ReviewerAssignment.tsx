import { ChevronDown } from "lucide-react";
import { useState } from "react";
import type { ReviewController } from "../../hooks/useReviewTasks";
import { useAuth } from "../../state/AuthContext";
import type { Project } from "../../types/models";
import type { ReviewTask } from "../../types/reviews";
import { can } from "../../utils/permissions";
import { ProjectAssigneeDialog } from "./ProjectAssigneeDialog";

export function ReviewerAssignment({ project, task, controller }: {
  project: Project; task: ReviewTask; controller: ReviewController;
}) {
  const { user, session } = useAuth();
  const [open, setOpen] = useState(false);
  const revision = task.latest_revision;
  const active = !!revision && ["submitted", "under_review"].includes(revision.status);
  const manage = can(project, user, "review.assign");
  const mine = !!user && revision?.reviewer_id === user.id;
  const assignSelf = !!user && !revision?.reviewer_id && can(project, user, "review.view");
  const editable = active && (manage || assignSelf);
  const assign = async (id: string) => {
    try { await controller.action(task, "assign", { reviewer_id: id }); }
    catch (err) { await controller.refresh().catch(() => {}); throw err; }
  };

  return <div className="reviewer-assignment">
    <span className="reviewer-label">Reviewer</span>
    {editable ? <button className="reviewer-picker-trigger" aria-label={revision?.reviewer_id ? "Change reviewer" : "Assign reviewer"} onClick={() => setOpen(true)}>
      <strong>{revision?.reviewer_name || "Choose a reviewer"}</strong><ChevronDown size={14}/>
    </button> : <strong>{revision?.reviewer_name || "Unassigned"}</strong>}
    {mine && <span className="muted">You</span>}
    {open && editable && <ProjectAssigneeDialog key={revision.id} projectId={project.uuid} role="reviewer"
      currentId={revision.reviewer_id} selfId={user?.id} selfOnly={!manage} allowSharedQueue={manage}
      excludedId={session?.workflow.independent_reviewer && revision.submitted_by_id!==user?.id ? revision.submitted_by_id : undefined}
      description={manage ? "Type a name to find a reviewer, including yourself." : "Type your name to assign this review to yourself."}
      onAssign={assign} onClose={() => setOpen(false)} />}
  </div>;
}
