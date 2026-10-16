import type { Dispatch, SetStateAction } from "react";
import { useEffect, useRef, useState } from "react";
import type { Frame } from "../types/models";
import { startFramePlayback, type PlaybackImage } from "../utils/framePlayback";
export { playbackDelay } from "../utils/framePlayback";

async function loadImage(url: string, signal: AbortSignal): Promise<PlaybackImage> {
  const response = await fetch(url, { signal });
  if (!response.ok) throw new Error(`Frame request failed (${response.status})`);
  const blob = await response.blob();
  signal.throwIfAborted();
  const objectUrl = URL.createObjectURL(blob);
  const image = new Image();
  const abort = () => { image.src = ""; };
  signal.addEventListener("abort", abort, { once: true });
  try {
    image.src = objectUrl;
    await image.decode();
    signal.throwIfAborted();
    return { url: objectUrl, dispose: () => { image.src = ""; URL.revokeObjectURL(objectUrl); } };
  } catch (error) {
    image.src = "";
    URL.revokeObjectURL(objectUrl);
    throw error;
  } finally {
    signal.removeEventListener("abort", abort);
  }
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
  const visible = useRef<PlaybackImage | null>(null);
  const currentFrame = useRef(frameNumber);
  currentFrame.current = frameNumber;
  const errorHandler = useRef(onError);
  errorHandler.current = onError;
  const [displayed, setDisplayed] = useState<{ frame: number; source: typeof imageUrl; url: string } | null>(null);
  useEffect(() => () => { visible.current?.dispose(); }, []);
  useEffect(() => {
    if (!playing) return;
    return startFramePlayback({
      frames,
      start: currentFrame.current,
      loadImage: imageUrl ? (number, signal) => loadImage(imageUrl(number), signal) : undefined,
      onFrame: (number, image) => {
        if (image) {
          visible.current?.dispose();
          visible.current = image;
          setDisplayed({ frame: number, source: imageUrl, url: image.url });
        }
        setFrameNumber(number);
      },
      onEnd: () => setPlaying(false),
      onError: number => {
        setPlaying(false);
        errorHandler.current(`Unable to load frame ${number + 1}. Retry the frame preview before playing.`);
      },
    });
  }, [playing, frames, imageUrl, setFrameNumber, setPlaying]);
  return displayed?.frame === frameNumber && displayed.source === imageUrl ? displayed.url : undefined;
}
