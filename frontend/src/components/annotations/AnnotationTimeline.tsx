import { ArrowLeft, ArrowRight, ChevronFirst, ChevronLast, Flag, Pause, Play } from "lucide-react";
import type { AnnotationEditorController } from "../../hooks/useAnnotationEditor";
import { timestamp } from "../../utils/recordings";

export function AnnotationTimeline({ editor }: {  
editor: Pick<AnnotationEditorController,
    | "frameNumber"
    | "playing"
    | "frames"
    | "frame"
    | "frameStep"
    | "keyframeNumbers"
    | "togglePlayback"
    | "reviewedCount"
    | "commit"
    | "navigateFrame"
  >
}) {
  const {
    frameNumber,
    playing,
    frames,
    frame,
    frameStep,
    keyframeNumbers,
    togglePlayback,
    reviewedCount,
    commit,
    navigateFrame
  } = editor;
  return (<div className="timeline">
    <div className="timeline-top">
      <div className="playback-controls">
        <button
          className="icon-button"
          aria-label="First frame"
          disabled={frameNumber === 0}
          onClick={() => navigateFrame(0)}
        >
          <ChevronFirst size={18} />
        </button>
        <button
          className="icon-button"
          aria-label={`Back ${frameStep} frames`} title={`Back ${frameStep} frames (←)`}
          disabled={frameNumber === 0}
          onClick={() => navigateFrame(frameNumber - frameStep)}
        >
          <ArrowLeft size={17} />
        </button>
        <button
          className="play-button"
          aria-label={playing ? "Pause playback" : "Play frames"}
          disabled={frames.length < 2}
          onClick={togglePlayback}
        >
          {playing ? (
            <Pause size={17} fill="currentColor" />
          ) : (
            <Play size={17} fill="currentColor" />
          )}
        </button>
        <button
          className="icon-button"
          aria-label={`Forward ${frameStep} frames`} title={`Forward ${frameStep} frames (→)`}
          disabled={frameNumber >= frames.length - 1}
          onClick={() => navigateFrame(frameNumber + frameStep)}
        >
          <ArrowRight size={17} />
        </button>
        <button
          className="icon-button"
          aria-label="Last frame"
          disabled={frameNumber >= frames.length - 1}
          onClick={() => navigateFrame(frames.length - 1)}
        >
          <ChevronLast size={18} />
        </button>
      </div>
      <label className="frame-number">
        <span>Step</span><input aria-label="Frame step size" type="number" min="1" step="1" value={frameStep} onChange={(event) => {
          const value = Number(event.target.value);
          if (Number.isSafeInteger(value) && value > 0) commit((next) => { next.settings.step = value; });
        }} /><span>frames</span>
      </label>
      <label className="frame-number">
        <span>Frame</span>
        <input
          aria-label="Frame number"
          type="number"
          min="1"
          max={frames.length}
          value={frameNumber + 1}
          onChange={(event) =>
            navigateFrame(Number(event.target.value) - 1)
          }
        />
        <span>/ {frames.length}</span>
      </label>
    </div>
    <div className="marked-timeline">
      <input className="frame-slider" aria-label="Frame seek" type="range" min="0" max={Math.max(0, frames.length - 1)} value={frameNumber} onChange={(event) => navigateFrame(Number(event.target.value))} />
      <div className="timeline-markers" aria-label="Keyframe markers">
        {keyframeNumbers.map((number) => (
          <button key={number} className={`keyframe-marker ${number === frameNumber ? "current" : ""}`} style={{ left: `${frames.length > 1 ? number / (frames.length - 1) * 100 : 0}%` }} aria-label={`Jump to keyframe ${number + 1}`} title={`Keyframe ${number + 1}`} aria-pressed={number === frameNumber} onClick={() => navigateFrame(number)}><Flag size={13} fill="currentColor" /></button>
        ))}
      </div>
    </div>
    <p className="timeline-keyframe-help">{keyframeNumbers.length} keyframes · Click a marker to jump to its frame.</p>
    <div className="timeline-bottom">
      <code>{timestamp(frame?.time_ns)}</code>
      <span>
        {reviewedCount.toLocaleString()} /{" "}
        {frames.length.toLocaleString()} marked complete
      </span>
    </div>
  </div>);
}
