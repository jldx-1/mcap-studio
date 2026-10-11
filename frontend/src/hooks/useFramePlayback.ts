import type { Dispatch, SetStateAction } from "react";
import { useEffect, useRef, useState } from "react";
import type { Frame } from "../types/models";

export function playbackDelay(frames: Frame[], number: number) {
  let delay = 120;
  try {
    delay = Number(BigInt(frames[number + 1].time_ns) - BigInt(frames[number].time_ns)) / 1_000_000;
  } catch { /* Use the fallback when timestamps are absent. */ }
  // A gap in a recording should not leave the play button apparently stuck.
  return Math.max(16, Math.min(1000, delay));
}

export function useFramePlayback({ playing, frameNumber, frames, imageUrl, setFrameNumber, setPlaying, onError }: {
  playing: boolean;
  frameNumber: number;
  frames: Frame[];
  imageUrl?: (number: number) => string;
  setFrameNumber: Dispatch<SetStateAction<number>>;
  setPlaying: Dispatch<SetStateAction<boolean>>;
  onError: (message: string) => void;
}) {
  const currentUrl = useRef<string | null>(null);
  const [displayed, setDisplayed] = useState<{ frame: number; source: typeof imageUrl; url: string } | null>(null);
  useEffect(() => () => { if (currentUrl.current) URL.revokeObjectURL(currentUrl.current); }, []);
  useEffect(() => {
    if (!playing) return;
    if (frameNumber >= frames.length - 1) { setPlaying(false); return; }
    let active = true;
    let timer: number | undefined;
    let pendingUrl: string | null = null;
    let committed = false;
    const controller = new AbortController();
    const started = performance.now();
    const delay = playbackDelay(frames, frameNumber);
    const next = frameNumber + 1;
    const image = imageUrl ? new Image() : null;
    const advance = () => {
      if (!active) return;
      timer = window.setTimeout(() => {
        if (!active) return;
        if (pendingUrl) {
          if (currentUrl.current) URL.revokeObjectURL(currentUrl.current);
          currentUrl.current = pendingUrl;
          committed = true;
          setDisplayed({ frame: next, source: imageUrl, url: pendingUrl });
        }
        setFrameNumber(next);
      }, Math.max(0, delay - (performance.now() - started)));
    };
    if (image && imageUrl) {
      image.onload = advance;
      image.onerror = () => {
        if (active) { setPlaying(false); onError(`Unable to load frame ${next + 1}. Retry the frame preview before playing.`); }
      };
      // Display the downloaded image directly; SVG does not need a second
      // network request, including when HTTP caching is disabled.
      void fetch(imageUrl(next), { signal: controller.signal }).then(async (response) => {
        if (!response.ok) throw new Error(`Frame request failed (${response.status})`);
        const blob = await response.blob();
        if (!active) return;
        pendingUrl = URL.createObjectURL(blob);
        image.src = pendingUrl;
      }).catch(() => {
        if (active) { setPlaying(false); onError(`Unable to load frame ${next + 1}. Retry the frame preview before playing.`); }
      });
    } else { advance(); }
    return () => {
      active = false;
      controller.abort();
      window.clearTimeout(timer);
      if (image) { image.onload = null; image.onerror = null; image.src = ""; }
      if (pendingUrl && !committed) URL.revokeObjectURL(pendingUrl);
    };
  }, [playing, frameNumber, frames, imageUrl, setFrameNumber, setPlaying, onError]);
  return displayed?.frame === frameNumber && displayed.source === imageUrl ? displayed.url : undefined;
}
