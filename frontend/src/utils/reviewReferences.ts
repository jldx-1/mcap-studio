import type { Frame } from "../types/models";

// Review timestamps are elapsed time from the first frame of the selected channel.
export function elapsedSeconds(frames: Frame[], frame: number): number | null {
  try {
    if (!frames[frame] || !frames[0]) return null;
    return Number(BigInt(frames[frame].time_ns) - BigInt(frames[0].time_ns)) / 1e9;
  } catch { return null; }
}

export function formatReviewTime(seconds: number): string {
  const milliseconds = Math.round(seconds * 1000);
  return `${Math.floor(milliseconds / 60000)}:${(Math.floor(milliseconds / 1000) % 60).toString().padStart(2, "0")}.${(milliseconds % 1000).toString().padStart(3, "0")}`;
}

export function parseReviewTime(value: string): number | null {
  if (!/^\d+(?::[0-5]\d){0,2}(?:\.\d{1,9})?$/.test(value.trim())) return null;
  return value.trim().split(":").reduce((total, part) => total * 60 + Number(part), 0);
}

export function frameAtTime(frames: Frame[], seconds: number): number | null {
  const duration = elapsedSeconds(frames, frames.length - 1);
  if (duration === null || !Number.isFinite(seconds) || seconds < 0 || seconds > duration) return null;
  let low = 0, high = frames.length - 1;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    const time = elapsedSeconds(frames, middle);
    if (time === null) return null;
    if (time < seconds) low = middle + 1;
    else high = middle;
  }
  const previous = low > 0 ? elapsedSeconds(frames, low - 1) : null;
  return previous !== null && seconds - previous <= elapsedSeconds(frames, low)! - seconds ? low - 1 : low;
}

export interface ReviewPosition { channel: string; frame: number }
