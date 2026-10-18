import { TopicErrors } from "./TopicErrors";
import { useAuth } from "../../state/AuthContext";
import { can } from "../../utils/permissions";
import { Pencil } from "lucide-react";
import { annotationPatch, applySavedAnnotations } from "../../annotations/state";
import { patch } from "../../services/http";
import type { AnnotationState, Project, Recording, SavedAnnotationPatch, Topic } from "../../types/models";
import type { Perform } from "../../types/workspace";
import { filename, recordingPath, topicCounts } from "../../utils/recordings";

const formats = (topic: Topic) => topic.kind === "image"
  ? [["yolo", "YOLO boxes"], ["classification", "Video classification"]]
  : [["csv", "CSV columns"]];

export function TopicTable({
  project,
  recording,
  busy,
  perform,
  onRecording,
  onEdit,
  onManageTranslations,
}: {
  project: Project;
  recording: Recording;
  busy: boolean;
  perform: Perform;
  onRecording: (recording: Recording) => void;
  onEdit: (channel: string) => void;
  onManageTranslations: () => void;
}) {
  const { user } = useAuth();
  const translate = can(project,user,"project.manage");
  const editable = can(project,user,"annotation.edit");
  const annotator = editable || can(project,user,"annotation.create");
  const update = (channel: string, kind: "format" | "list", value: string) =>
    void perform("Saving topic settings", async () => {
      const next = structuredClone(recording.annotations);
      if (kind === "format") next.techniques[channel] = value;
      else {
        next.topic_label_lists ||= {};
        next.topic_labels ||= {};
        if (value) {
          next.topic_label_lists[channel] = value;
          next.topic_labels[channel] = project.label_lists.find(
            (list) => list.uuid === value,
          )!.label_ids;
        } else {
          delete next.topic_label_lists[channel];
          delete next.topic_labels[channel];
        }
      }
      const saved = await patch<AnnotationState | SavedAnnotationPatch>(
        `${recordingPath(project.uuid, recording.uuid)}/annotations`,
        annotationPatch(recording.annotations, next),
      );
      onRecording({ ...recording, annotations: applySavedAnnotations(next, saved) });
    });
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>Data labeling table</h2>
          <p>
            {recording.topics.length} topics in{" "}
            {filename(recording.source_file)}
          </p>
        </div>
        <span className="pill">
          {recording.topics
            .reduce((sum, topic) => sum + topic.frame_count, 0)
            .toLocaleString()}{" "}
          messages
        </span>
      </div>
      <div className="table-wrap">
        <table className="topic-table">
          <thead>
            <tr>
              <th>Topic</th>
              <th>Labeling format</th>
              <th>Label list</th>
              <th>Annotations</th>
              <th>Approved samples</th>
              <th>Labels</th>
            </tr>
          </thead>
          <tbody>
            {recording.topics.map((topic) => {
              const id = String(topic.channel_id);
              const counts = topicCounts(recording, id);
              const csvDatasets = project.datasets.filter((dataset) => dataset.format === "csv" && dataset.source_recording_uuid === recording.uuid && dataset.source_channel === id);
              const actionLabel = !annotator && topic.kind === "image" ? "View annotations" : topic.kind !== "image" ? "Open CSV Workspace" : counts.annotated || counts.reviewed ? "Edit Labels" : "Create Labels";
              const saved = recording.annotations.techniques[id];
              const technique = topic.kind === "image" ? (saved === "classification" ? "classification" : "yolo") : "csv";
              return (
                <tr key={id}>
                  <td>
                    <strong className="topic-name">{topic.topic}</strong>
                    <span className="table-secondary">
                      Channel {id} · {topic.kind} ·{" "}
                      {topic.frame_count.toLocaleString()} messages
                    </span>
                    {!!topic.errors?.length && <TopicErrors topic={topic.topic} channel={id} errors={topic.errors}/>}
                  </td>
                  <td>
                    <select
                      aria-label={`Labeling format for ${topic.topic}`}
                      value={technique}
                      disabled={
                        !editable || busy || counts.annotated > 0 || !topic.frame_count || topic.kind !== "image"
                      }
                      onChange={(event) =>
                        update(id, "format", event.target.value)
                      }
                    >
                      <option value="" disabled>
                        Choose format…
                      </option>
                      {formats(topic).map(([value, label]) => (
                        <option value={value} key={value}>
                          {label}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    {topic.kind === "image" ? (
                      <select
                        aria-label={`Label list for ${topic.topic}`}
                        value={
                          recording.annotations.topic_label_lists?.[id] || ""
                        }
                        disabled={!editable || busy || !topic.frame_count || topic.kind !== "image"}
                        onChange={(event) =>
                          update(id, "list", event.target.value)
                        }
                      >
                        <option value="">Choose a label list…</option>
                        {project.label_lists.map((list) => (
                          <option key={list.uuid} value={list.uuid}>
                            {list.name}
                          </option>
                        ))}
                      </select>
                    ) : <span className="muted">Custom columns</span>}
                  </td>
                  <td className="numeric">
                    {topic.kind === "image" ? counts.annotated.toLocaleString() : `${csvDatasets.length} CSV datasets`}
                  </td>
                  <td>
                    {topic.kind === "image" ? <span className={`pill ${counts.reviewed ? "blue" : ""}`}>
                      {counts.reviewed.toLocaleString()} / {topic.frame_count.toLocaleString()}
                    </span> : <span className="pill">{Math.max(0, ...csvDatasets.map((dataset) => dataset.entry_count)).toLocaleString()} rows saved</span>}
                  </td>
                  <td>
                    <button
                      className="button primary small"
                      aria-label={`${actionLabel} for ${topic.topic}`}
                      disabled={busy || !topic.frame_count || (topic.kind !== "image" && !can(project,user,"annotation.view") && !can(project,user,"review.view"))}
                      onClick={() => onEdit(id)}
                    >
                      <Pencil size={15} /> {actionLabel}
                    </button>
                  </td>
                </tr>

              );
            })}
          </tbody>
        </table>
      </div>
      {translate&&<div className="collection-body"><button className="button secondary small" disabled={busy} onClick={onManageTranslations}>Manage labels &amp; translations</button></div>}
      {!recording.topics.length && (
        <p className="collection-body muted">This MCAP has no topics.</p>
      )}
    </section>
  );
}
