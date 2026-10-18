import type { RecordingSummary } from "../types/models";
import type { ReviewScope, ReviewTask } from "../types/reviews";
export const libraryStatuses=["Imported","Needs labeling","Labeling","In review","Changes requested","Partially approved","Approved"] as const;
export type LibraryStatus=typeof libraryStatuses[number];
export function recordingFamilies(recordings:RecordingSummary[]) {
  const byId=new Map(recordings.map(item=>[item.uuid,item])),roots=new Map<string,string>();
  const root=(item:RecordingSummary)=>{
    if(roots.has(item.uuid))return roots.get(item.uuid)!;
    let value=item;const seen=new Set<string>();
    while(value.source_recording_uuid&&!seen.has(value.uuid)) {seen.add(value.uuid);const parent=byId.get(value.source_recording_uuid);if(!parent)break;value=parent;}
    roots.set(item.uuid,value.uuid);return value.uuid;
  };
  const groups=new Map<string,RecordingSummary[]>();
  for(const item of recordings){const id=root(item);groups.set(id,[...(groups.get(id)||[]),item]);}
  return [...groups.values()].map(members=>({members,item:[...members].sort((a,b)=>{
    if(!!a.source_recording_uuid!==!!b.source_recording_uuid)return a.source_recording_uuid?-1:1;
    return (Date.parse(b.created_at||"")||0)-(Date.parse(a.created_at||"")||0);
  })[0]}));
}
export function coveredFrames(scopes:ReviewScope[],channel:string,count:number) {
  const intervals=scopes.filter(scope=>scope.channel_id===channel).map(scope=>[Math.max(0,scope.start_frame),Math.min(count-1,scope.end_frame)]).filter(([start,end])=>end>=start).sort((a,b)=>a[0]-b[0]);
  let covered=0,last=-1;
  for(const [start,end] of intervals){covered+=Math.max(0,end-Math.max(last,start-1));last=Math.max(last,end);}
  return covered;
}
export function libraryWorkflow(recording:RecordingSummary,tasks:ReviewTask[]) {
  const current=tasks.filter(task=>task.recording_id===recording.uuid);
  const required=new Map<string,number>();
  const coverage=recording.summary?.annotation_topics;
  if(coverage){for(const [channel,value] of Object.entries(coverage))if(value.required&&value.frame_count)required.set(channel,value.frame_count);}
  else {for(const topic of recording.topic_details||[])if(topic.modality==="Video"&&topic.message_count)required.set(topic.channel_id,topic.message_count);}
  for(const task of current)for(const scope of task.scopes)if(!required.has(scope.channel_id)) {
    const count=coverage?.[scope.channel_id]?.frame_count??recording.topic_details?.find(topic=>topic.channel_id===scope.channel_id)?.message_count??scope.end_frame+1;
    if(count)required.set(scope.channel_id,count);
  }
  const scopes=current.filter(task=>(task.latest_revision?.status||task.status)==="approved"&&!task.working_dirty).flatMap(task=>task.latest_revision?.scopes||task.scopes);
  const total=[...required.values()].reduce((sum,count)=>sum+count,0);
  const approved=[...required].reduce((sum,[channel,count])=>sum+coveredFrames(scopes,channel,count),0);
  const labeled=coverage?Object.values(coverage).reduce((sum,topic)=>sum+topic.annotated_count,0):Math.max(recording.summary?.annotated_count||0,recording.summary?.reviewed_count||0,...current.map(task=>task.annotated_count));
  const statuses=current.map(task=>task.latest_revision?.status||task.status);
  let status:LibraryStatus;
  if(statuses.includes("changes_requested"))status="Changes requested";
  else if(statuses.some(status=>status==="submitted"||status==="under_review"))status="In review";
  else if(total>0&&approved>=total&&!current.some(task=>task.working_dirty))status="Approved";
  else if(approved>0)status="Partially approved";
  else if(labeled>0||current.some(task=>task.working_dirty||task.status==="in_progress"))status="Labeling";
  else status=required.size?"Needs labeling":"Imported";
  return {status,total,approved,labeled};
}
export function libraryDuration(seconds:number|undefined) {
  if(seconds===undefined)return "Unavailable";
  const whole=Math.round(seconds),hours=Math.floor(whole/3600);
  return `${hours?`${hours}:`:""}${(Math.floor(whole/60)%60).toString().padStart(2,"0")}:${(whole%60).toString().padStart(2,"0")}`;
}
export function libraryFileSize(bytes:number|undefined) {
  if(bytes===undefined)return "Unavailable";
  const units=["B","KiB","MiB","GiB","TiB"],index=Math.min(units.length-1,Math.floor(Math.log(Math.max(1,bytes))/Math.log(1024)));
  return `${(bytes/1024**index).toLocaleString(undefined,{maximumFractionDigits:index?1:0})} ${units[index]}`;
}
