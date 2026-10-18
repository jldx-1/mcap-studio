import { useMemo } from "react";
import { ArrowLeft, ArrowRight, ChevronFirst, ChevronLast, Flag, Pause, Play } from "lucide-react";
import type { AnnotationEditorController } from "../../hooks/useAnnotationEditor";
import { annotationTimeline } from "../../utils/annotationTimeline";
import { colors } from "../../annotations/state";
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
    | "commit"
    | "state"
    | "channel"
    | "selectedBox"
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
    commit,
    navigateFrame
  } = editor;
  const {coverage,tracks}=useMemo(()=>annotationTimeline(editor.state,editor.channel,frames.length),[editor.state,editor.channel,frames.length]);
  const coverageLabels={labeled:"Labeled",complete:"Complete",empty:"Empty",skipped:"Skipped"};
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
          title={playing ? "Pause playback" : "Play at recorded frame timing (1×)"}
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
        <span>Frame step</span><input aria-label="Frame step size" type="number" min="1" step="1" value={frameStep} onChange={(event) => {
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
    <div className="annotation-timeline-heading"><strong>Annotation timeline</strong><span>Frame {frameNumber+1} / {frames.length}</span></div>
    <div className="marked-timeline">
      <input className="frame-slider" aria-label="Frame seek" type="range" min="0" max={Math.max(0, frames.length - 1)} value={frameNumber} onChange={(event) => navigateFrame(Number(event.target.value))} />
      <div className="timeline-markers" aria-label="Keyframe markers">
        {keyframeNumbers.map((number) => (
          <button key={number} className={`keyframe-marker ${number === frameNumber ? "current" : ""}`} style={{ left: `${frames.length > 1 ? number / (frames.length - 1) * 100 : 0}%` }} aria-label={`Jump to keyframe ${number + 1}`} title={`Keyframe ${number + 1}`} aria-pressed={number === frameNumber} onClick={() => navigateFrame(number)}><Flag size={13} fill="currentColor" /></button>
        ))}
      </div>
    </div>
    <div className="annotation-coverage" aria-label="Annotation coverage" title="Annotation coverage" onClick={event=>{const rect=event.currentTarget.getBoundingClientRect();navigateFrame(Math.round((event.clientX-rect.left)/rect.width*Math.max(0,frames.length-1)));}}>
      {coverage.map(span=><span key={`${span.start}-${span.kind}`} className={`coverage-span ${span.kind}`} title={`${coverageLabels[span.kind]} · Frames ${span.start+1}–${span.end+1}`} style={{left:`${span.start/frames.length*100}%`,width:`${(span.end-span.start+1)/frames.length*100}%`}}/>)}
      <span className="coverage-playhead" style={{left:`${frameNumber/Math.max(1,frames.length-1)*100}%`}}/>
    </div>
    <div className="timeline-legend"><span className="legend-keyframe">◆ Keyframe</span><span className="legend-labeled">● Labeled</span>{coverage.some(span=>span.kind==="complete")&&<span>● Complete</span>}{coverage.some(span=>span.kind==="empty")&&<span>○ Empty</span>}{coverage.some(span=>span.kind==="skipped")&&<span>● Skipped</span>}<span>{keyframeNumbers.length} keyframes</span></div>
    {!!tracks.length&&<div className="timeline-tracks" aria-label="Object track spans">{tracks.map(track=><div className={`timeline-track ${editor.selectedBox?.track_id===track.id?"selected":""}`} key={track.id}><span title={editor.state.classes[track.classId]}>#{track.id} {editor.state.classes[track.classId]}</span><div className="track-span-lane"><button className="track-span" aria-label={`Track ${track.id}, frames ${track.start+1} to ${track.end+1}`} title={`Track ${track.id} · Frames ${track.start+1}–${track.end+1}`} style={{left:`${track.start/frames.length*100}%`,width:`${(track.end-track.start+1)/frames.length*100}%`,background:colors[track.classId%colors.length]}} onClick={event=>{const rect=event.currentTarget.parentElement!.getBoundingClientRect();navigateFrame(event.detail?Math.max(track.start,Math.min(track.end,Math.round((event.clientX-rect.left)/rect.width*(frames.length-1)))):track.start);}}/></div></div>)}</div>}
    <div className="timeline-bottom">
      <code>{timestamp(frame?.time_ns)}</code>
    </div>
  </div>);
}
