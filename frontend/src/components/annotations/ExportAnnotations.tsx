import { useState } from "react";
import { post } from "../../services/http";
import type { ExportResult, Project, Recording } from "../../types/models";
import type { Perform } from "../../types/workspace";
import { filename } from "../../utils/recordings";
import Dialog from "../shared/Dialog";

export function ExportAnnotations({
  project,
  recording,
  busy,
  perform,
  onNotice,
  onClose,
  error,
}: {
  project: Project;
  recording: Recording;
  busy: boolean;
  perform: Perform;
  onNotice: (notice: string) => void;
  onClose: () => void;
  error: string;
}) {
  const [format, setFormat] = useState("coco");
  const [channel, setChannel] = useState("");
  const [result, setResult] = useState<ExportResult | null>(null);
  return (
    <Dialog title="Export approved annotations" onClose={onClose} error={error}>
      <p className="modal-description">
        Export saved, approved data from {filename(recording.source_file)}.
      </p>
      <label>
        Topic
        <select
          aria-label="Export topic"
          value={channel}
          onChange={(event) => setChannel(event.target.value)}
        >
          <option value="">All topics</option>
          {recording.topics.map((topic) => (
            <option key={topic.channel_id} value={topic.channel_id}>
              {topic.topic}
            </option>
          ))}
        </select>
      </label>
      <label>
        Format
        <select
          aria-label="Export format"
          value={format}
          onChange={(event) => setFormat(event.target.value)}
        >
          <option value="coco">COCO · object detection</option>
          <option value="yolo">YOLO · object detection</option>
          <option value="jsonl">JSONL · records & classification</option>
        </select>
      </label>
      <div className="modal-footer">
        <button className="button secondary" disabled={busy} onClick={onClose}>
          Close
        </button>
        <button
          className="button primary"
          disabled={busy}
          onClick={() =>
            void perform("Exporting annotations", async () => {
              setResult(
                await post<ExportResult>(`/projects/${project.uuid}/exports`, {
                  recording_uuid: recording.uuid,
                  ...(channel ? { channel_id: channel } : {}),
                  format,
                }),
              );
              onNotice("Reviewed annotations exported.");
            })
          }
        >
          Export {format.toUpperCase()}
        </button>
      </div>
      {result && (
        <div className="export-result">
          <strong>Export complete</strong>
          <code>{result.path}</code>
          {result.download_url && (
            <a
              className="button secondary small"
              href={result.download_url}
              download
            >
              Download ZIP
            </a>
          )}
        </div>
      )}
    </Dialog>
  );
}
