import type {
  Dispatch,
  MutableRefObject,
  PointerEvent as ReactPointerEvent,
  RefObject,
  SetStateAction
} from 'react';
import type { AnnotationState, Box, Frame, FrameAnnotation } from '../types/models';
import type { Tracks } from './state';
import { clone, rebuildTrack, trackInfo } from './state';

export interface PointerDrag {
  kind: 'draw' | 'move' | 'resize';
  x: number;
  y: number;
  box?: Box;
  index?: number;
  corner?: string;
}

interface AnnotationToolContext {
  svg: RefObject<SVGSVGElement | null>;
  drag: MutableRefObject<PointerDrag | null>;
  width: number;
  height: number;
  boxTool: boolean;
  availableClasses: { name: string; id: number }[];
  classId: number;
  selectedBox: Box | undefined | null;
  preview: Box | null;
  frameRow: FrameAnnotation;
  technique: string;
  setTool: Dispatch<SetStateAction<"draw" | "select">>;
  stateRef: MutableRefObject<AnnotationState>;
  channel: string;
  frameNumber: number;
  frames: Frame[];
  commit: (edit: (value: AnnotationState) => void) => void;
  setPlaying: Dispatch<SetStateAction<boolean>>;
  setSelected: Dispatch<SetStateAction<number | null>>;
  setPreview: Dispatch<SetStateAction<Box | null>>;
}

export function createAnnotationTools({
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
}: AnnotationToolContext) {
  const point = (event: ReactPointerEvent) => {
    const p = svg.current!.createSVGPoint();
    p.x = event.clientX;
    p.y = event.clientY;
    const local = p.matrixTransform(svg.current!.getScreenCTM()!.inverse());
    return {
      x: Math.max(0, Math.min(width, local.x)),
      y: Math.max(0, Math.min(height, local.y)),
    };
  };
  const start = (
    event: ReactPointerEvent,
    kind: "draw" | "move" | "resize",
    index?: number,
    corner?: string,
  ) => {
    if (!boxTool || !availableClasses.length || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    setPlaying(false);
    const location = point(event);
    const box = index === undefined ? undefined : clone(frameRow.boxes[index]);
    drag.current = { kind, ...location, box, index, corner };
    setSelected(index ?? null);
    setPreview(box || { ...location, width: 0, height: 0, class_id: classId });
    svg.current?.setPointerCapture(event.pointerId);
  };
  const move = (event: ReactPointerEvent) => {
    const current = drag.current;
    if (!current) return;
    const location = point(event);
    if (current.kind === "draw")
      setPreview({
        x: Math.min(current.x, location.x),
        y: Math.min(current.y, location.y),
        width: Math.abs(location.x - current.x),
        height: Math.abs(location.y - current.y),
        class_id: classId,
      });
    else if (current.kind === "move" && current.box)
      setPreview({
        ...current.box,
        x: Math.max(
          0,
          Math.min(
            width - current.box.width,
            current.box.x + location.x - current.x,
          ),
        ),
        y: Math.max(
          0,
          Math.min(
            height - current.box.height,
            current.box.y + location.y - current.y,
          ),
        ),
      });
    else if (current.box) {
      const box = current.box;
      const left = current.corner?.includes("w")
        ? Math.min(location.x, box.x + box.width - 1)
        : box.x;
      const right = current.corner?.includes("e")
        ? Math.max(location.x, box.x + 1)
        : box.x + box.width;
      const top = current.corner?.includes("n")
        ? Math.min(location.y, box.y + box.height - 1)
        : box.y;
      const bottom = current.corner?.includes("s")
        ? Math.max(location.y, box.y + 1)
        : box.y + box.height;
      setPreview({
        ...box,
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
      });
    }
  };
  const keyframe = (box: Box) =>
    commit((next) => {
      next.techniques[channel] = technique;
      const allIds = Object.values(next.annotations[channel] || {}).flatMap(
        (row) => row.boxes.map((item) => item.track_id || 0),
      );
      const id =
        box.track_id ||
        Math.max(
          0,
          ...allIds,
          ...Object.keys(
            (next.tracks as Tracks | undefined)?.[channel] || {},
          ).map(Number),
        ) + 1;
      const manual = { ...box, track_id: id, origin: "manual" };
      const info = trackInfo(next, channel, id) || {
        start: frameNumber,
        end: null,
        keyframes: {},
      };
      if (info.end !== null && frameNumber > info.end) info.end = frameNumber;
      Object.values(info.keyframes).forEach((value) => {
        value.class_id = manual.class_id;
      });
      info.keyframes[frameNumber] = manual;
      info.start = Math.min(info.start, frameNumber);
      rebuildTrack(next, channel, id, info, frames);
    });
  const end = (event: ReactPointerEvent) => {
    const current = drag.current;
    if (!current) return;
    drag.current = null;
    svg.current?.releasePointerCapture(event.pointerId);
    if (preview && preview.width >= 1 && preview.height >= 1) {
      keyframe(preview);
      setTool("select");
      const id = preview.track_id;
      const nextBoxes =
        stateRef.current.annotations[channel]?.[frameNumber]?.boxes || [];
      setSelected(
        id
          ? nextBoxes.findIndex((box) => box.track_id === id)
          : nextBoxes.length - 1,
      );
    }
    setPreview(null);
  };
  const editSelected = (values: Partial<Box>) => {
    if (!selectedBox) return;
    const box = { ...selectedBox, ...values };
    box.width = Math.max(1, Math.min(width, box.width));
    box.height = Math.max(1, Math.min(height, box.height));
    box.x = Math.max(0, Math.min(width - box.width, box.x));
    box.y = Math.max(0, Math.min(height - box.height, box.y));
    keyframe(box);
    setSelected(
      stateRef.current.annotations[channel][frameNumber].boxes.findIndex(
        (value) => value.track_id === box.track_id,
      ),
    );
  };


  return { start, move, end, editSelected };
}
