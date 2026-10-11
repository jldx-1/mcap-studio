import type { Recording } from "../types/models";

export const recordingPath = (project: string, recording: string) =>
  `/projects/${encodeURIComponent(project)}/recordings/${encodeURIComponent(recording)}`;

export const framesFor = (recording: Recording, channel: string) =>
  recording.index?.topics?.[channel]?.frames ?? [];

export function filename(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

export function timestamp(value: string | number | undefined) {
  if (value === undefined) return "—";
  try {
    const time = BigInt(value);
    return `${time / 1_000_000_000n}.${(time % 1_000_000_000n).toString().padStart(9, "0")} s`;
  } catch {
    return String(value);
  }
}

export function formatDuration(seconds: number) {
  const minutes = Math.floor(seconds / 60);
  return minutes >= 60 ? `${Math.floor(minutes / 60)}h ${minutes % 60}m` : `${minutes}m ${Math.floor(seconds % 60)}s`;
}

export const topicCounts = (recording: Recording, channel: string) => {
  const rows = Object.values(recording.annotations.annotations[channel] || {});
  return {
    annotated: rows.filter(
      (row) =>
        row.boxes?.length ||
        row.labels?.length ||
        (row.fields && JSON.stringify(row.fields) !== "{}"),
    ).length,
    reviewed: recording.eligible_counts?.[channel] ?? 0,
  };
};
