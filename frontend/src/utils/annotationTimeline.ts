import type { AnnotationState } from "../types/models";
import type { Tracks } from "../annotations/state";
export type CoverageKind="labeled"|"complete"|"empty"|"skipped";
export interface CoverageSpan {start:number;end:number;kind:CoverageKind}
export function annotationTimeline(state:AnnotationState,channel:string,count:number) {
  const coverage:CoverageSpan[]=[],tracks=new Map<number,{id:number;start:number;end:number;classId:number}>();
  const stored=(state.tracks as Tracks|undefined)?.[channel]||{};
  for(const [id,track] of Object.entries(stored)){
    const first=Object.values(track.keyframes)[0];
    if(first&&track.start<count)tracks.set(Number(id),{id:Number(id),start:Math.max(0,track.start),end:Math.min(count-1,track.end??count-1),classId:first.class_id});
  }
  for(const [key,row] of Object.entries(state.annotations[channel]||{}).sort((a,b)=>Number(a[0])-Number(b[0]))) {
    const number=Number(key);if(!Number.isInteger(number)||number<0||number>=count)continue;
    const kind:CoverageKind|undefined=row.skipped?"skipped":row.explicit_empty?"empty":row.boxes?.length||row.labels?.length||(row.fields&&JSON.stringify(row.fields)!=="{}")?"labeled":row.reviewed?"complete":undefined;
    if(kind){const last=coverage.at(-1);if(last&&last.kind===kind&&last.end===number-1)last.end=number;else coverage.push({start:number,end:number,kind});}
    for(const box of row.boxes||[]){if(!box.track_id||stored[String(box.track_id)])continue;const track=tracks.get(box.track_id);if(track)track.end=number;else tracks.set(box.track_id,{id:box.track_id,start:number,end:number,classId:box.class_id});}
  }
  return {coverage,tracks:[...tracks.values()].filter(track=>track.end>=track.start).sort((a,b)=>a.id-b.id)};
}
export function submissionSummary(state:AnnotationState,channel:string,count:number,ranges:{start_frame:number;end_frame:number}[]) {
  const intervals=ranges.map(range=>[Math.max(0,range.start_frame),Math.min(count-1,range.end_frame)]).filter(([a,b])=>b>=a).sort((a,b)=>a[0]-b[0]);
  let total=0,last=-1;for(const [start,end] of intervals){total+=Math.max(0,end-Math.max(last,start-1));last=Math.max(last,end);}
  let labeled=0,boxes=0;const classes=new Set<number>();
  for(const [key,row] of Object.entries(state.annotations[channel]||{})){
    const number=Number(key);if(!Number.isInteger(number)||!intervals.some(([start,end])=>number>=start&&number<=end)||row.skipped)continue;
    const objects=row.boxes||[],labels=row.labels||[];
    if(objects.length||labels.length||(row.fields&&JSON.stringify(row.fields)!=="{}"))labeled++;
    boxes+=objects.length;objects.forEach(box=>classes.add(box.class_id));labels.forEach(id=>classes.add(id));
  }
  return {total,labeled,boxes,classes:classes.size};
}
