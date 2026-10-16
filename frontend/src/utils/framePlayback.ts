import type { Frame } from "../types/models";

/** Subtract nanoseconds before converting so epoch timestamps stay exact. */
export function playbackDelay(frames: Frame[], number: number): number {
  try {
    const delta = BigInt(frames[number + 1].time_ns) - BigInt(frames[number].time_ns);
    if (delta >= 0n) return Number(delta) / 1_000_000;
  } catch { /* Missing or invalid timestamps use the preview fallback. */ }
  return 120;
}

export interface PlaybackImage {
  url: string;
  dispose: () => void;
}

interface Result { image?: PlaybackImage; error?: unknown }
interface BufferedFrame {
  ready: Promise<Result>;
  resolve: (result: Result) => void;
  result?: Result;
  loading: boolean;
}

/** A bounded player shared by labeling and review. Ownership of displayed
 * images transfers to onFrame; unused prefetched images are disposed on stop. */
export function startFramePlayback({
  frames, start, loadImage, onFrame, onEnd, onError,
  now = () => performance.now(),
  schedule = (callback, delay) => window.setTimeout(callback, delay),
  cancel = handle => window.clearTimeout(handle),
}: {
  frames: Frame[];
  start: number;
  loadImage?: (number: number, signal: AbortSignal) => Promise<PlaybackImage>;
  onFrame: (number: number, image?: PlaybackImage) => void;
  onEnd: () => void;
  onError: (number: number, error: unknown) => void;
  now?: () => number;
  schedule?: (callback: () => void, delay: number) => number;
  cancel?: (handle: number) => void;
}): () => void {
  const controller = new AbortController();
  const buffer = new Map<number, BufferedFrame>();
  let stopped = false;
  let inFlight = 0;
  let current = start;
  let queued = start + 1;
  let deadline = 0;
  let timer: number | undefined;

  const stop = () => {
    if (stopped) return;
    stopped = true;
    controller.abort();
    if (timer !== undefined) cancel(timer);
    for (const entry of buffer.values()) entry.result?.image?.dispose();
    buffer.clear();
  };
  const pump = () => {
    if (stopped || !loadImage) return;
    for (const [number, entry] of buffer) {
      if (inFlight >= 3) break;
      if (entry.loading) continue;
      entry.loading = true;
      inFlight++;
      void Promise.resolve().then(() => loadImage(number, controller.signal)).then(image => {
        if (stopped) { image.dispose(); return; }
        entry.result = { image };
        entry.resolve(entry.result);
      }).catch(error => {
        entry.result = { error };
        entry.resolve(entry.result);
      }).finally(() => { inFlight--; pump(); });
    }
  };
  const fill = () => {
    if (!loadImage) return;
    // Estimate decoded pixels plus encoded data. Always allow one large frame,
    // but cap ordinary lookahead at eight frames / roughly 64 MiB.
    let bytes = [...buffer.keys()].reduce((sum, number) => sum + frameBytes(number), 0);
    while (queued < frames.length && buffer.size < 8) {
      const cost = frameBytes(queued);
      if (buffer.size && bytes + cost > 64 * 1024 ** 2) break;
      let resolve!: (result: Result) => void;
      const ready = new Promise<Result>(done => { resolve = done; });
      buffer.set(queued++, { ready, resolve, loading: false });
      bytes += cost;
    }
    pump();
  };
  const frameBytes = (number: number) => {
    const frame = frames[number];
    return frame.width && frame.height ? frame.width * frame.height * 8 : 8 * 1024 ** 2;
  };
  const next = () => {
    if (stopped) return;
    if (current >= frames.length - 1) { stop(); onEnd(); return; }
    deadline += playbackDelay(frames, current);
    const show = async () => {
      if (stopped) return;
      const remaining = deadline - now();
      if (remaining > 0) {
        // Browser timers overflow above ~24 days; retain longer recorded gaps.
        timer = schedule(() => { void show(); }, Math.min(remaining, 2_147_483_647));
        return;
      }
      const number = current + 1;
      const entry = buffer.get(number);
      let result = entry?.result;
      if (entry && !result) {
        const waiting = now();
        result = await entry.ready;
        // Buffering pauses the recording clock; do not rush through unseen
        // frames to catch up after a slow request.
        deadline += now() - waiting;
      }
      if (stopped) return;
      if (result && "error" in result) { stop(); onError(number, result.error); return; }
      buffer.delete(number);
      current = number;
      onFrame(number, result?.image);
      fill();
      next();
    };
    // Keep one continuous deadline rather than adding render/load time to each
    // frame interval. Late browser callbacks therefore do not accumulate drift.
    timer = schedule(() => { void show(); }, Math.min(Math.max(0, deadline - now()), 2_147_483_647));
  };

  if (start >= frames.length - 1) { onEnd(); return stop; }
  fill();
  // Buffer a short window before starting the recording clock.
  void Promise.all([...buffer.values()].map(entry => entry.ready)).then(() => {
    if (stopped) return;
    deadline = now();
    next();
  });
  return stop;
}
