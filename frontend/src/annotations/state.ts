import type { AnnotationState, Box, Frame, FrameAnnotation, Project, Recording, SavedAnnotationPatch } from "../types/models";

export interface Track {
  start: number;
  end: number | null;
  keyframes: Record<string, Box>;
}

export type Tracks = Record<string, Record<string, Track>>;

export const colors = [
  "#0866ff",
  "#20b78a",
  "#b078ff",
  "#ff8b40",
  "#ec5890",
  "#14b8cd",
];

export const clone = <T,>(value: T): T => structuredClone(value);

export const freshRow = (): FrameAnnotation => ({
  reviewed: false,
  boxes: [],
  labels: [],
  excluded_tracks: [],
});

export function initialState(project: Project, recording: Recording): AnnotationState {
  const state = clone(recording.annotations || emptyState(recording));
  state.classes ||= [];
  state.annotations ||= {};
  state.techniques ||= {};
  state.settings ||= { positions: {}, step: 1 };
  const ids = (state.class_uuids ||= state.classes.map(() => null));
  project.labels.forEach((label) => {
    let index = ids.indexOf(label.uuid);
    if (index < 0) index = state.classes.indexOf(label.name);
    if (index < 0) {
      state.classes.push(label.name);
      ids.push(label.uuid);
    } else {
      state.classes[index] = label.name;
      ids[index] = label.uuid;
    }
  });
  return state;
}

export function trackInfo(
  state: AnnotationState,
  channel: string,
  id: number,
): Track | undefined {
  const stored = (state.tracks as Tracks | undefined)?.[channel]?.[String(id)];
  if (stored) return clone(stored);
  const keyframes: Record<string, Box> = {};
  for (const [number, row] of Object.entries(state.annotations[channel] || {}))
    for (const box of row.boxes || [])
      if (box.track_id === id && (box.origin || "manual") === "manual")
        keyframes[number] = clone(box);
  const anchors = Object.keys(keyframes).map(Number);
  return anchors.length
    ? { start: Math.min(...anchors), end: null, keyframes }
    : undefined;
}

export function timeRatio(
  frames: Frame[],
  start: number,
  end: number,
  number: number,
) {
  try {
    const a = BigInt(frames[start].time_ns),
      b = BigInt(frames[end].time_ns),
      t = BigInt(frames[number].time_ns);
    if (a !== b) return Number(t - a) / Number(b - a);
  } catch {
    /* use frame positions */
  }
  return end === start ? 0 : (number - start) / (end - start);
}

export function rebuildTrack(
  state: AnnotationState,
  channel: string,
  id: number,
  track: Track,
  frames: Frame[],
) {
  const rows = (state.annotations[channel] ||= {});
  const anchors = Object.entries(track.keyframes)
    .map(([key, box]) => [Number(key), box] as const)
    .sort((a, b) => a[0] - b[0]);
  if (!anchors.length) return;
  for (const row of Object.values(rows))
    row.boxes = (row.boxes || []).filter((box) => box.track_id !== id);
  for (
    let number = track.start;
    number <= Math.min(track.end ?? frames.length - 1, frames.length - 1);
    number++
  ) {
    const row = (rows[number] ||= freshRow());
    const manual = track.keyframes[number];
    if (!manual && (row.explicit_empty || row.excluded_tracks?.includes(id)))
      continue;
    let box: Box;
    if (manual) {
      box = clone(manual);
      delete row.explicit_empty;
      row.excluded_tracks = (row.excluded_tracks || []).filter(
        (value) => value !== id,
      );
    } else {
      const left =
        [...anchors].reverse().find(([frame]) => frame <= number) || anchors[0];
      const right = anchors.find(([frame]) => frame > number);
      box = { ...left[1], origin: right ? "interpolate" : "copy" };
      if (right) {
        const ratio = timeRatio(frames, left[0], right[0], number);
        for (const key of ["x", "y", "width", "height"] as const)
          box[key] += (right[1][key] - left[1][key]) * ratio;
      }
    }
    const width = frames[number].width || 1,
      height = frames[number].height || 1;
    box.width = Math.min(box.width, width);
    box.height = Math.min(box.height, height);
    box.x = Math.max(0, Math.min(box.x, width - box.width));
    box.y = Math.max(0, Math.min(box.y, height - box.height));
    row.boxes.push(box);
  }
  const tracks = (state.tracks ||= {}) as Tracks;
  (tracks[channel] ||= {})[id] = clone(track);
}

export const emptyState = (recording: Recording): AnnotationState => ({
  format_version: 1,
  sha256: recording.sha256,
  classes: [],
  techniques: {},
  annotations: {},
  settings: { step: 1, positions: {} },
});

export function annotationPatch(previous: AnnotationState, next: AnnotationState) {
  const changes: { channel_id: string; frame: number; annotation: FrameAnnotation | null; expected: FrameAnnotation | null }[] = [];
  for (const channel of new Set([...Object.keys(previous.annotations), ...Object.keys(next.annotations)])) {
    const oldRows = previous.annotations[channel] || {};
    const newRows = next.annotations[channel] || {};
    for (const frame of new Set([...Object.keys(oldRows), ...Object.keys(newRows)])) {
      if (JSON.stringify(oldRows[frame]) !== JSON.stringify(newRows[frame]))
        changes.push({ channel_id: channel, frame: Number(frame), annotation: newRows[frame] ?? null, expected: oldRows[frame] ?? null });
    }
  }
  const metadata: Record<string, unknown> = {};
  const expected_metadata: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(next)) {
    if (!["annotations", "sha256", "format_version"].includes(key) && JSON.stringify(previous[key]) !== JSON.stringify(value)) {
      metadata[key] = value;
      expected_metadata[key] = previous[key] ?? null;
    }
  }
  return { sha256: next.sha256, changes, metadata, expected_metadata, return_delta: true };
}

export function applySavedAnnotations(next: AnnotationState, saved: AnnotationState | SavedAnnotationPatch): AnnotationState {
  if ("format_version" in saved) return saved;
  const state = { ...next, ...saved.metadata };
  for (const change of saved.changes) {
    const rows = state.annotations[change.channel_id] ||= {};
    if (change.annotation === null) delete rows[change.frame];
    else rows[change.frame] = change.annotation;
  }
  return state;
}
