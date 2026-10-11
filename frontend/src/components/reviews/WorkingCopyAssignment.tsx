import { Pencil } from "lucide-react";
import { useState } from "react";
import type { ReviewController } from "../../hooks/useReviewTasks";
import { useAuth } from "../../state/AuthContext";
import type { Project } from "../../types/models";
import type { ReviewTask } from "../../types/reviews";
import { can } from "../../utils/permissions";
import { ProjectAssigneeDialog } from "./ProjectAssigneeDialog";

export function WorkingCopyAssignment({ project, task, controller }: {
  project: Project; task: ReviewTask; controller: ReviewController;
}) {
  const { user } = useAuth();
  const [open, setOpen] = useState(false);
  const editable = can(project, user, "review.assign");
  const initials = task.annotator_name.trim().split(/\s+/).slice(0, 2).map((part) => part[0]).join("").toLocaleUpperCase();
  const assign = async (id: string) => {
    try { await controller.action(task, "assign_annotator", { annotator_id: id }); }
    catch (err) { await controller.refresh().catch(() => {}); throw err; }
  };

  return <div className="working-copy-assignment" role="group" aria-label="Working copy assignment">
    <span className="working-copy-avatar" aria-hidden="true">{initials || "?"}</span>
    <div className="working-copy-owner"><span>Working copy</span><strong>{task.annotator_name}</strong><small>Assigned annotator</small></div>
    {editable && <button className="button secondary small" aria-label="Change annotator" onClick={() => setOpen(true)}><Pencil size={14} />Change</button>}
    {open && editable && <ProjectAssigneeDialog projectId={project.uuid} role="annotator" currentId={task.annotator_id}
      description="Choose who will edit the working copy and submit its next revision."
      onAssign={assign} onClose={() => setOpen(false)} />}
  </div>;
}
