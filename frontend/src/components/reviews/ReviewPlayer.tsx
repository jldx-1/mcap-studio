import { useCallback, useEffect, useState } from "react";
import { useFramePlayback } from "../../hooks/useFramePlayback";
import { api } from "../../services/http";
import type { Project, Recording } from "../../types/models";
import type { ReviewTask, ReviewRevision } from "../../types/reviews";
import { framesFor, recordingPath } from "../../utils/recordings";
import { elapsedSeconds, formatReviewTime, type ReviewPosition } from "../../utils/reviewReferences";
import { colors } from "../../annotations/state";

export function ReviewPlayer({ project,recording,task,revision,jump,onError,onBeginReview,onPositionChange }: { project: Project;recording: Recording;task: ReviewTask;revision:ReviewRevision;jump: { channel: string;frame: number;key: number } | null;onError: (message: string) => void;onBeginReview?:()=>void;onPositionChange?:(position:ReviewPosition)=>void }) {
  const [scopeIndex,setScopeIndex] = useState(0);const scope = task.scopes[scopeIndex] || task.scopes[0];
  const channel = scope.channel_id;
  const [number,setNumber] = useState(scope.start_frame);const [playing,setPlaying] = useState(false);
  const [sample,setSample] = useState<unknown>(null);const [sampleLoading,setSampleLoading] = useState(false);
  const frames = framesFor(recording,channel);const frame = frames[number];
  const lastFrame = Math.max(0,frames.length-1);
  const submittedFrame = task.scopes.some((section)=>section.channel_id===channel&&number>=section.start_frame&&number<=section.end_frame);
  const topic = recording.topics.find((t) => String(t.channel_id) === channel);
  const base = recordingPath(project.uuid,recording.uuid);
  const imageUrl = useCallback((n: number) => `/api${base}/frames/${channel}/${n}`,[base,channel]);
  const display = useFramePlayback({ playing,frameNumber:number,frames,imageUrl:topic?.kind === "image" ? imageUrl : undefined,setFrameNumber:setNumber,setPlaying,onError });
  useEffect(() => { setNumber(jump && jump.channel === scope.channel_id && jump.frame >= 0 && jump.frame <= lastFrame ? jump.frame : scope.start_frame);setPlaying(false); },[scope.start_frame,scope.end_frame,scope.channel_id,scopeIndex,jump,lastFrame]);
  useEffect(() => {
    if(!jump) return;
    const matched = task.scopes.findIndex((s) => s.channel_id === jump.channel && jump.frame >= s.start_frame && jump.frame <= s.end_frame);
    const index = matched >= 0 ? matched : task.scopes.findIndex((s) => s.channel_id === jump.channel && jump.frame >= 0 && jump.frame < framesFor(recording,s.channel_id).length);
    if(index >= 0) { setScopeIndex(index);setNumber(jump.frame);setPlaying(false); }
  },[jump,task.scopes,recording]);
  useEffect(() => {
    const frozenCsv=submittedFrame&&(!!task.dataset_id||recording.revision_datasets?.some((d)=>d.source_channel===channel));
    if(topic?.kind === "image" && !frozenCsv) { setSample(null);return; }
    let active=true;setSampleLoading(true);
    const path = frozenCsv ? `/projects/${project.uuid}/tasks/${task.id}/sample?channel_id=${channel}&frame=${number}&revision=${revision.number}` : `${base}/records/${channel}/${number}`;
    void api(path).then((value) => { if(active) setSample(value); }).catch((err) => { if(active) { setSample(null);onError(String(err)); } }).finally(() => { if(active) setSampleLoading(false); });
    return () => { active=false; };
  },[project.uuid,task.id,task.dataset_id,revision.number,base,channel,number,topic?.kind,onError,submittedFrame]);
  useEffect(()=>{onPositionChange?.({channel,frame:number});},[channel,number,onPositionChange]);
  const time=elapsedSeconds(frames,number);
  const row = submittedFrame ? recording.annotations.annotations[channel]?.[number] : undefined;
  const annotated = Object.entries(recording.annotations.annotations[channel] || {}).filter(([n,row]) => task.scopes.some((section)=>section.channel_id===channel&&+n>=section.start_frame&&+n<=section.end_frame) && (row.reviewed || row.boxes?.length || row.labels?.length || row.fields)).map(([n]) => +n).sort((a,b) => a-b);
  const seek = (n: number) => { setPlaying(false);setNumber(Math.max(0,Math.min(lastFrame,n))); };
  return <section className="panel review-player" data-revision={revision.number}><div className="panel-heading"><div><h2>{topic?.kind === "image" ? "Video" : "Sensor data"}</h2><p>{topic?.topic} · {frames.length} frames{task.scopes.some((section)=>section.channel_id===channel&&(section.start_frame>0||section.end_frame<lastFrame))&&` · Submitted ${task.scopes.filter((section)=>section.channel_id===channel).map((section)=>`${section.start_frame+1}–${section.end_frame+1}`).join(", ")}`}</p></div>{task.scopes.length>1 && <label>Topic or section<select value={scopeIndex} onChange={(e) => setScopeIndex(Number(e.target.value))}>{task.scopes.map((s,i) => <option key={i} value={i}>Channel {s.channel_id} · {s.start_frame+1}–{s.end_frame+1}</option>)}</select></label>}</div>
    <div className="collection-body">{!submittedFrame&&<p className="field-help">Outside the submitted frame ranges · Recording context</p>}{topic?.kind === "image" ? <svg className="review-video" viewBox={`0 0 ${frame?.width || 1} ${frame?.height || 1}`} role="img" aria-label={`Submitted frame ${number+1}`}><image href={display || imageUrl(number)} width={frame?.width} height={frame?.height} />{row?.boxes?.map((box,i) => <g key={i}><rect x={box.x} y={box.y} width={box.width} height={box.height} fill="none" stroke={colors[box.class_id % colors.length]} strokeWidth={Math.max(1,(frame?.width || 100)/400)} /><text x={box.x} y={Math.max(10,box.y-3)} fill={colors[box.class_id % colors.length]} fontSize={Math.max(8,(frame?.width || 100)/70)}>{recording.annotations.classes[box.class_id]}</text></g>)}</svg> : <pre className="record-preview">{sampleLoading ? "Loading message…" : JSON.stringify(sample,null,2)}</pre>}
      <div className="button-row"><button className="button secondary small" disabled={number <= 0} onClick={() => seek(number-1)}>Previous</button><button className="button primary small" aria-label={playing ? "Pause review playback" : "Play review video"} title={playing ? "Pause playback" : "Play at recorded frame timing (1×)"} disabled={frames.length <= 1} onClick={() => { if(!playing)onBeginReview?.();if(number>=lastFrame) setNumber(0);setPlaying((p) => !p); }}>{playing ? "Pause" : "Play"}</button><button className="button secondary small" disabled={number>=lastFrame} onClick={() => seek(number+1)}>Next</button><label>Frame<input aria-label="Review frame number" className="compact-input" type="number" min={1} max={lastFrame+1} value={number+1} onChange={(e) => seek(Number(e.target.value)-1)} /></label><span className="muted">/ {frames.length}{time!==null&&` · @ ${formatReviewTime(time)}`}</span></div>
      <input className="frame-slider" type="range" aria-label="Review timeline" min={0} max={lastFrame} value={number} onChange={(e) => seek(Number(e.target.value))} />
      <div className="button-row"><button className="button secondary small" disabled={!annotated.some((n) => n<number)} onClick={() => seek(annotated.filter((n) => n<number).at(-1)!)}>Previous annotated frame</button><button className="button secondary small" disabled={!annotated.some((n) => n>number)} onClick={() => seek(annotated.find((n) => n>number)!)}>Next annotated frame</button><span>{row?.boxes?.length || 0} boxes · {(row?.labels || []).map((id) => recording.annotations.classes[id]).join(", ") || "No classification labels"}</span></div>
      {row?.fields != null && <pre>{JSON.stringify(row.fields,null,2)}</pre>}
    </div>
  </section>;
}
