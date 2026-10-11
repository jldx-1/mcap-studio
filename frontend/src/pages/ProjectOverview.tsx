import { ArrowRight, Database, Layers3, Tag } from "lucide-react";
import type { Project } from "../types/models";
import { useAuth } from "../state/AuthContext";
import { can } from "../utils/permissions";
import { formatDuration } from "../utils/recordings";

export function ProjectOverview({ project, busy, onOpen }: { project: Project; busy: boolean; onOpen: (page: "mcaps" | "labels" | "datasets") => void }) {
  const { user } = useAuth();
  const summaries = project.mcaps.flatMap((item) => item.summary ? [item.summary] : []);
  const modalities = [...new Set(summaries.flatMap((item) => item.modalities))];
  const topics = new Set(summaries.flatMap((item) => item.topic_names)).size;
  const labelMemberships = new Map<string, number>();
  for (const list of project.label_lists) {
    for (const id of new Set(list.label_ids)) {
      labelMemberships.set(id, (labelMemberships.get(id) || 0) + 1);
    }
  }
  const populatedLists = project.label_lists.filter((list) => list.label_ids.length > 0).length;
  const sharedLabels = [...labelMemberships.values()].filter((count) => count > 1).length;
  return (
    <>
      <div className="project-hub">
        <button className="panel hub-card" disabled={busy || !can(project,user,"mcap.view")} onClick={() => onOpen("mcaps")}>
          <Layers3 size={24} /><h2>Project MCAPs</h2><p>Recording{project.mcaps.length === 1 ? "" : "s"}</p>
          <span>{formatDuration(summaries.reduce((sum, item) => sum + item.duration_seconds, 0))} total duration · {topics} distinct topics</span>
          <span>{modalities.length ? modalities.join(" · ") : "No indexed modalities yet"}</span>
          <span className="hub-link">View MCAPs <ArrowRight size={16} /></span>
        </button>
        <button className="panel hub-card" disabled={busy || !can(project,user,"labels.manage")} onClick={() => onOpen("labels")}>
          <Tag size={24} /><h2>Project Labels</h2>
          <p>{project.label_lists.length.toLocaleString()} label lists · {labelMemberships.size.toLocaleString()} individual labels</p>
          <span>{populatedLists.toLocaleString()} populated lists · {(project.label_lists.length - populatedLists).toLocaleString()} empty lists</span>
          <span>{sharedLabels.toLocaleString()} labels shared across multiple lists</span>
          <span className="hub-link">Manage labels <ArrowRight size={16} /></span>
        </button>
        <button className="panel hub-card" disabled={busy} onClick={() => onOpen("datasets")}>
          <Database size={24} /><h2>Project Datasets</h2><p>Saved datasets</p>
          <span>{project.datasets.reduce((sum, item) => sum + item.entry_count, 0).toLocaleString()} total entries</span>
          <span>Explore included MCAPs, labels, and modalities</span>
          <span className="hub-link">View datasets <ArrowRight size={16} /></span>
        </button>
      </div>
      {summaries.length < project.mcaps.length && <p className="field-help">Metrics cover {summaries.length} of {project.mcaps.length} indexed MCAPs. Open a recording to index its data, then return to refresh the overview.</p>}
    </>
  );
}
