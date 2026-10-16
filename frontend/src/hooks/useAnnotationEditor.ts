import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Tracks } from "../annotations/state";
import { annotationPatch, applySavedAnnotations, clone, freshRow, initialState, trackInfo } from "../annotations/state";
import { createAnnotationTools } from "../annotations/tools";
import { api, patch } from "../services/http";
import type { AnnotationEditorProps } from "../types/editor";
import type { AnnotationState, Box, FrameAnnotation, SavedAnnotationPatch } from "../types/models";
import { framesFor, recordingPath } from "../utils/recordings";
import { useFramePlayback } from "./useFramePlayback";

export function useAnnotationEditor({
  readOnly = false,
  project,
  recording,
  channel,
  onBack,
  onLabels,
  onError,
  onNotice,
  onSaved,
  registerNavigationGuard
}: AnnotationEditorProps) {

  const [state, setState] = useState(() => { const initial = initialState(project, recording); initial.settings.step = 1; if (["coco", "pascal_voc", "kitti", "mot", "cvat"].includes(initial.techniques[channel])) initial.techniques[channel] = "yolo"; return initial; });
  const [frameNumber, setFrameNumber] = useState(() =>
    Number(recording.annotations?.settings?.positions?.[channel] || 0),
  );
  const [selected, setSelected] = useState<number | null>(null);
  const [classId, setClassId] = useState(0);
  const [tool, setTool] = useState<"draw" | "select">("draw");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [leavePrompt, setLeavePrompt] = useState(false);
  const [saveError, setSaveError] = useState("");
  const leaveResolver = useRef<((value: boolean) => void) | null>(null);
  const leavePromise = useRef<Promise<boolean> | null>(null);
  const savingRef = useRef(false);
  const [playing, setPlaying] = useState(false);
  const [history, setHistory] = useState<AnnotationState[]>([]);
  const [record, setRecord] = useState<unknown>(null);
  const [recordLoading, setRecordLoading] = useState(false);
  const [imageFailed, setImageFailed] = useState(false);
  const [fields, setFields] = useState("{}");
  const [preview, setPreview] = useState<Box | null>(null);
  const [help, setHelp] = useState(false);
  const svg = useRef<SVGSVGElement>(null);
  const drag = useRef<{
    kind: "draw" | "move" | "resize";
    x: number;
    y: number;
    box?: Box;
    index?: number;
    corner?: string;
  } | null>(null);
  const stateRef = useRef(state);
  const savedRef = useRef(JSON.stringify(state));
  const fieldsRef = useRef(fields);
  fieldsRef.current = fields;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  stateRef.current = state;
  const frames = framesFor(recording, channel);
  const frame = frames[frameNumber];
  const savedStep = Number(state.settings.step);
  const frameStep = Number.isSafeInteger(savedStep) && savedStep > 0 ? savedStep : 1;
  const keyframeNumbers = useMemo(() => {
    const numbers = new Set<number>();
    for (const info of Object.values((state.tracks as Tracks | undefined)?.[channel] || {}))
      Object.keys(info.keyframes).forEach((number) => numbers.add(Number(number)));
    for (const [number, row] of Object.entries(state.annotations[channel] || {})) {
      if (row.boxes?.some((box) => (box.origin || "manual") === "manual") || row.labels?.length || (row.fields && JSON.stringify(row.fields) !== "{}")) numbers.add(Number(number));
    }
    return [...numbers].filter((number) => Number.isInteger(number) && number >= 0 && number < frames.length).sort((a, b) => a - b);
  }, [state, channel, frames.length]);
  const topic = recording.topics.find(
    (item) => String(item.channel_id) === channel,
  );
  const isImage = topic?.kind === "image";
  const frameRow = state.annotations[channel]?.[frameNumber] || freshRow();
  const technique =
    state.techniques[channel] && state.techniques[channel] !== "undefined"
      ? state.techniques[channel]
      : isImage
        ? "yolo"
        : "jsonl";
  const boxTool = isImage && !["classification", "jsonl"].includes(technique);
  const base = recordingPath(project.uuid, recording.uuid);
  const frameUrl = `/api${base}/frames/${channel}/${frameNumber}`;
  const playbackImage = useCallback((number: number) => `/api${base}/frames/${channel}/${number}`, [base, channel]);
  const displayedFrameUrl = useFramePlayback({ playing, frameNumber, frames, imageUrl: isImage ? playbackImage : undefined, setFrameNumber, setPlaying, onError });
  const togglePlayback = () => {
    if (!playing && frameNumber >= frames.length - 1) setFrameNumber(0);
    setPlaying((value) => !value);
  };
  const width = frame?.width || 1,
    height = frame?.height || 1;
  const selectedBox = selected === null ? null : frameRow.boxes[selected];
  const track = selectedBox?.track_id
    ? trackInfo(state, channel, selectedBox.track_id)
    : undefined;
  const reviewedCount = useMemo(
    () =>
      Object.values(state.annotations[channel] || {}).filter(
        (value) => value.reviewed,
      ).length,
    [state, channel],
  );
  const assignedList = recording.annotations.topic_label_lists?.[channel] || "";
  const assignedLabelList = project.label_lists.find((list) => list.uuid === assignedList);
  const allowedIds = assignedLabelList?.label_ids || [];
  const availableClasses = state.classes
    .map((name, id) => ({ name, id }))
    .filter(
      (item) =>
        !assignedList || allowedIds.includes(state.class_uuids?.[item.id] || ""),
    )
    .sort((a, b) => allowedIds ? allowedIds.indexOf(state.class_uuids?.[a.id] || "") - allowedIds.indexOf(state.class_uuids?.[b.id] || "") : a.id - b.id);

  // Adding shared labels in the dialog must preserve the current annotation draft.
  useEffect(() => {
    const next = initialState(project, {
      ...recording,
      annotations: stateRef.current,
    });
    const baseline = initialState(project, {
      ...recording,
      annotations: JSON.parse(savedRef.current) as AnnotationState,
    });
    savedRef.current = JSON.stringify(baseline);
    stateRef.current = next;
    setState(next);
  }, [project.labels]);

  useEffect(() => {
    setFrameNumber(
      Math.min(
        Number(stateRef.current.settings.positions?.[channel] || 0),
        Math.max(0, frames.length - 1),
      ),
    );
    setSelected(null);
    setPlaying(false);
  }, [channel, frames.length]);
  useEffect(() => {
    setSelected(null);
    setImageFailed(false);
    setPreview(null);
    drag.current = null;
    setFields(
      JSON.stringify(
        stateRef.current.annotations[channel]?.[frameNumber]?.fields ?? {},
        null,
        2,
      ),
    );
  }, [channel, frameNumber]);
  useEffect(() => {
    if (!dirty) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);
  useEffect(() => {
    if (isImage || !frame) {
      setRecord(null);
      return;
    }
    let active = true;
    setRecordLoading(true);
    void api(`${base}/records/${channel}/${frameNumber}`)
      .then((value) => {
        if (active) setRecord(value);
      })
      .catch((err) => {
        if (active) onError(String(err));
      })
      .finally(() => {
        if (active) setRecordLoading(false);
      });
    return () => {
      active = false;
    };
  }, [base, channel, frameNumber, isImage, frame, onError]);
  useEffect(() => {
    if (!availableClasses.some((item) => item.id === classId))
      setClassId(availableClasses[0]?.id || 0);
  }, [classId, availableClasses]);

  const commit = useCallback((edit: (value: AnnotationState) => void) => {
    if(readOnly) return;
    const before = stateRef.current,
      next = clone(before);
    edit(next);
    stateRef.current = next;
    setHistory((value) => [...value.slice(-39), clone(before)]);
    setState(next);
    setDirty(JSON.stringify(next) !== savedRef.current);
  }, [readOnly]);
  const updateRow = (edit: (value: FrameAnnotation) => void) =>
    commit((next) => {
      next.techniques[channel] = technique;
      const row = ((next.annotations[channel] ||= {})[frameNumber] ||=
        freshRow());
      edit(row);
    });
  const undo = useCallback(() => {
    if(readOnly) return;
    const previous = history.at(-1);
    if (!previous) return;
    setHistory((value) => value.slice(0, -1));
    setState(previous);
    stateRef.current = previous;
    setDirty(JSON.stringify(previous) !== savedRef.current);
    setSelected(null);
  }, [history, readOnly]);
  const save = useCallback(async (silent = false) => {
    if(readOnly) return !dirtyRef.current;
    if (savingRef.current) return false;
    savingRef.current = true;
    setSaving(true);
    setSaveError("");
    onError("");
    try {
      const started = clone(stateRef.current);
      const value = clone(started);
      if (technique === "jsonl") {
        let parsed: unknown;
        try {
          parsed = JSON.parse(fieldsRef.current);
        } catch {
          throw new Error(
            "Annotation fields must be valid JSON before saving or changing the workspace.",
          );
        }
        const existing = value.annotations[channel]?.[frameNumber];
        if (existing || JSON.stringify(parsed) !== "{}")
          ((value.annotations[channel] ||= {})[frameNumber] ||=
            freshRow()).fields = parsed;
      }
      value.settings.positions = {
        ...value.settings.positions,
        [channel]: frameNumber,
      };
      if (
        !value.techniques[channel] ||
        value.techniques[channel] === "undefined"
      )
        value.techniques[channel] = technique;
      const saved = await patch<AnnotationState | SavedAnnotationPatch>(`${base}/annotations`, annotationPatch(JSON.parse(savedRef.current) as AnnotationState, value));
      const persisted = applySavedAnnotations(value, saved);
      const pending = annotationPatch(started, stateRef.current);
      const result = applySavedAnnotations(clone(persisted), { sha256: persisted.sha256, metadata: pending.metadata, changes: pending.changes });
      setState(result);
      stateRef.current = result;
      savedRef.current = JSON.stringify(persisted);
      const stillDirty = JSON.stringify(result) !== savedRef.current;
      setDirty(stillDirty);
      dirtyRef.current = stillDirty;
      onSaved(result);
      if (!silent) onNotice("Annotations saved.");
      return true;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setSaveError(message);
      onError(message);
      return false;
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [base, channel, frameNumber, technique, onError, onNotice, onSaved, readOnly]);
  useEffect(() => {
    if (!dirty || saving || readOnly || saveError || playing || drag.current) return;
    const timer = window.setTimeout(() => void save(true), 1200);
    return () => window.clearTimeout(timer);
  }, [state, fields, dirty, saving, readOnly, saveError, playing, save]);
  useEffect(() => {
    registerNavigationGuard(async () => {
      if (savingRef.current) return false;
      if (!dirtyRef.current) return true;
      setPlaying(false);
      if (!leavePromise.current) {
        setSaveError("");
        leavePromise.current = new Promise<boolean>((resolve) => {
          leaveResolver.current = resolve;
        });
        setLeavePrompt(true);
      }
      return leavePromise.current;
    });
    return () => registerNavigationGuard(null);
  }, [registerNavigationGuard]);
  useEffect(
    () => () => {
      leaveResolver.current?.(false);
    },
    [],
  );
  const resolveLeave = (value: boolean) => {
    if (savingRef.current) return;
    setLeavePrompt(false);
    leaveResolver.current?.(value);
    leaveResolver.current = null;
    leavePromise.current = null;
  };
  const navigateFrame = (number: number) => {
    setPlaying(false);
    if (!readOnly && technique === "jsonl") {
      let parsed: unknown;
      try {
        parsed = JSON.parse(fieldsRef.current);
      } catch {
        onError("Annotation fields must be valid JSON before changing frames.");
        return;
      }
      if (
        JSON.stringify(parsed) !==
        JSON.stringify(
          stateRef.current.annotations[channel]?.[frameNumber]?.fields ?? {},
        )
      )
        commit((next) => {
          ((next.annotations[channel] ||= {})[frameNumber] ||=
            freshRow()).fields = parsed;
        });
    }
    setFrameNumber(Math.max(0, Math.min(frames.length - 1, number)));
  };
  const deleteBox = useCallback(() => {
    if (selected === null) return;
    commit((next) => {
      const row = ((next.annotations[channel] ||= {})[frameNumber] ||=
        freshRow());
      const [box] = row.boxes.splice(selected, 1);
      if (box?.track_id)
        row.excluded_tracks = [
          ...new Set([...(row.excluded_tracks || []), box.track_id]),
        ];
    });
    setSelected(null);
  }, [channel, frameNumber, selected, commit]);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
      if ((event.ctrlKey || event.metaKey) && event.key === "s") {
        event.preventDefault();
        void save();
      } else if ((event.ctrlKey || event.metaKey) && event.key === "z") {
        event.preventDefault();
        undo();
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        navigateFrame(frameNumber + frameStep);
      } else if (event.key === "ArrowLeft") {
        event.preventDefault();
        navigateFrame(frameNumber - frameStep);
      } else if (event.key === " ") {
        if (target.closest("button, a")) return;
        event.preventDefault();
        if (frameNumber >= frames.length - 1) setFrameNumber(0);
        setPlaying((value) => !value);
      } else if (event.key.toLowerCase() === "b") setTool("draw");
      else if (event.key.toLowerCase() === "v") setTool("select");
      else if (event.key === "Delete" || event.key === "Backspace") {
        event.preventDefault();
        deleteBox();
      } else if (event.key.toLowerCase() === "r") {
        event.preventDefault();
        commit((next) => {
          const row = ((next.annotations[channel] ||= {})[frameNumber] ||=
            freshRow());
          row.reviewed = !row.reviewed;
          if (row.reviewed) { row.skipped = false; if (!row.boxes?.length && !row.labels?.length) row.explicit_empty = true; }
          else delete row.explicit_empty;
        });
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [save, undo, frames.length, deleteBox, commit, channel, frameNumber, frameStep]);
  const { start, move, end, editSelected } = createAnnotationTools({
    svg,
    drag,
    width,
    height,
    boxTool,
    availableClasses,
    classId,
    selectedBox,
    preview,
    stateRef,
    channel,
    frameNumber,
    frames,
    commit,
    setPlaying,
    setSelected,
    setPreview,
    frameRow,
    technique,
    setTool
  });
  return {
    readOnly,
    state,
    frameNumber,
    selected,
    setSelected,
    classId,
    setClassId,
    tool,
    setTool,
    dirty,
    setDirty,
    saving,
    leavePrompt,
    saveError,
    playing,
    history,
    record,
    recordLoading,
    imageFailed,
    setImageFailed,
    fields,
    setFields,
    preview,
    setPreview,
    help,
    setHelp,
    svg,
    drag,
    fieldsRef,
    dirtyRef,
    frames,
    frame,
    frameStep,
    keyframeNumbers,
    topic,
    isImage,
    frameRow,
    technique,
    boxTool,
    frameUrl,
    displayedFrameUrl,
    togglePlayback,
    width,
    height,
    selectedBox,
    track,
    reviewedCount,
    assignedLabelList,
    availableClasses,
    commit,
    updateRow,
    undo,
    save,
    resolveLeave,
    navigateFrame,
    deleteBox,
    start: readOnly ? () => {} : start,
    move: readOnly ? () => {} : move,
    end: readOnly ? () => {} : end,
    editSelected,
    channel,
    onBack,
    onLabels,
    onError,
    onNotice
  };
}

export type AnnotationEditorController = ReturnType<typeof useAnnotationEditor>;
