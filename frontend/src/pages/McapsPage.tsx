import { useState, type ReactNode } from "react";
import { ChevronLeft, ChevronRight, File, Search, Trash2, Upload } from "lucide-react";
import type { WorkspaceController } from "../hooks/useWorkspace";
import type { RecordingSummary } from "../types/models";
import { can } from "../utils/permissions";
import { filename } from "../utils/recordings";
import { useReviewTasks } from "../hooks/useReviewTasks";
import { latestRecordingTask, workflowStatus } from "../utils/reviews";
import { statusLabel } from "../types/reviews";

function duration(seconds: number | undefined) {
  if (seconds === undefined) return "—";
  const milliseconds=Math.round(seconds*1000),whole=Math.floor(milliseconds/1000),hours=Math.floor(whole/3600);
  return `${hours?`${hours}:`:""}${(Math.floor(whole/60)%60).toString().padStart(hours?2:1,"0")}:${(whole%60).toString().padStart(2,"0")}${seconds>0&&seconds<1?`.${(milliseconds%1000).toString().padStart(3,"0")}`:""}`;
}
function fileSize(bytes: number | undefined) {
  if (bytes === undefined) return "—";
  const units=["B","KiB","MiB","GiB","TiB"];
  const index=Math.min(units.length-1,Math.floor(Math.log(Math.max(1,bytes))/Math.log(1024)));
  return `${(bytes/1024**index).toLocaleString(undefined,{maximumFractionDigits:index?1:0})} ${units[index]}`;
}
export function McapsPage({ workspace, uploadButton }: { workspace: WorkspaceController; uploadButton: ReactNode }) {
  const {user,project,busy,setError,search,setSearch,dateSort,setDateSort,setRemoveTarget,chooseRecording,openReview}=workspace;
  const [page,setPage]=useState(0),[pageSize,setPageSize]=useState(10),[dragging,setDragging]=useState(false);
  const reviewAccess=can(project,user,"annotation.view")||can(project,user,"review.view");
  const reviews=useReviewTasks(project?.uuid||"",setError,!!project&&reviewAccess);
  if(!project)return null;
  const manage=can(project,user,"project.manage");
  const familyRoot=(recording:RecordingSummary)=>{
    let current=recording;const visited=new Set<string>();
    while(current.source_recording_uuid&&!visited.has(current.uuid)){
      visited.add(current.uuid);const parent=project.mcaps.find(item=>item.uuid===current.source_recording_uuid);
      if(!parent)break;current=parent;
    }
    return current.uuid;
  };
  const families=new Map<string,RecordingSummary[]>();
  for(const item of project.mcaps){const root=familyRoot(item);families.set(root,[...(families.get(root)||[]),item]);}
  const recordings=[...families.values()].filter(items=>items.some(item=>filename(item.source_file).toLowerCase().includes(search.trim().toLowerCase())))
    .map(items=>items.filter(item=>item.uuid!==familyRoot(item)).sort((a,b)=>(Date.parse(b.created_at||"")||0)-(Date.parse(a.created_at||"")||0))[0]||items[0])
    .sort((a,b)=>(dateSort==="newest"?-1:1)*((Date.parse(a.created_at||"")||0)-(Date.parse(b.created_at||"")||0)));
  const lastPage=Math.max(0,Math.ceil(recordings.length/pageSize)-1),current=Math.min(page,lastPage);
  const rows=recordings.slice(current*pageSize,(current+1)*pageSize);
  const uploadEnabled=manage&&!busy&&workspace.connected===true;
  return <div className="feature-stack recordings-page">
    {manage&&<section className={`recording-upload-zone ${dragging?"is-dragging":""}`} data-file-drop aria-label="Upload MCAP recordings" aria-disabled={!uploadEnabled}
      onDragEnter={event=>{event.preventDefault();if(uploadEnabled)setDragging(true);}}
      onDragOver={event=>{event.preventDefault();event.dataTransfer.dropEffect=uploadEnabled?"copy":"none";}}
      onDragLeave={event=>{if(!(event.relatedTarget instanceof Node)||!event.currentTarget.contains(event.relatedTarget))setDragging(false);}}
      onDrop={event=>{event.preventDefault();setDragging(false);if(uploadEnabled)void workspace.upload(Array.from(event.dataTransfer.files));}}>
      <span className="recording-upload-icon"><Upload size={26}/></span><div><h2>Drop your MCAP recordings here</h2><p>Upload one or more .mcap files to start labeling. Camera and sensor recordings are supported.</p></div>{uploadButton}
    </section>}
    <section className="panel"><div className="panel-heading"><div><h2>Recording library</h2><p>{recordings.length} {recordings.length===1?"recording":"recordings"}{search.trim()?" matching your search":" in this project"}</p></div></div>
      <div className="collection-body recording-table-controls"><label className="recording-search"><Search size={17}/><input type="search" aria-label="Search included MCAPs" placeholder="Search recordings…" value={search} onChange={event=>{setSearch(event.target.value);setPage(0);}}/></label><div className="button-row"><label>Sort by import date<select aria-label="Sort MCAPs by import date" value={dateSort} onChange={event=>{setDateSort(event.target.value);setPage(0);}}><option value="newest">Newest first</option><option value="oldest">Oldest first</option></select></label><label>Rows per page<select aria-label="Recordings per page" value={pageSize} onChange={event=>{setPageSize(Number(event.target.value));setPage(0);}}>{[10,20,30,40,50].map(size=><option value={size} key={size}>{size}</option>)}</select></label></div></div>
      <div className="table-wrap"><table className="recordings-table"><thead><tr><th>Recording</th><th>Topics</th><th>Length</th><th>File size</th><th>Status</th><th><span className="visually-hidden">Actions</span></th></tr></thead><tbody>
        {rows.map(item=>{
          const task=latestRecordingTask(reviews.tasks,item.uuid);
          return <tr key={item.uuid} className="recording-table-row"><td><button className="recording-name-button" aria-label={`Open recording ${filename(item.source_file)}`} disabled={!!busy} onClick={()=>void chooseRecording(item.uuid)}><File size={20}/><span><strong>{filename(item.source_file)}</strong>{item.created_at&&<small>{new Date(item.created_at).toLocaleString()}</small>}</span></button></td><td>{(item.topic_count??item.summary?.topic_count)?.toLocaleString()??"—"}</td><td>{duration(item.duration_seconds??item.summary?.duration_seconds)}</td><td>{fileSize(item.file_size_bytes)}</td><td>{reviewAccess&&task?.current_revision?<button className={`pill mcap-review-status ${workflowStatus(task)==="approved"?"green":"blue"}`} aria-label={`Review status for ${filename(item.source_file)}: ${statusLabel[workflowStatus(task)]}`} disabled={!!busy} onClick={()=>void openReview({taskId:task.id,view:"review"})}>{statusLabel[workflowStatus(task)]}</button>:<span className="muted">Not submitted</span>}</td><td>{manage&&<button className="icon-button danger-text" aria-label={`Remove ${filename(item.source_file)} from project`} disabled={!!busy} onClick={()=>{setError("");setRemoveTarget(item);}}><Trash2 size={16}/></button>}</td></tr>;
        })}
      </tbody></table></div>
      {!rows.length&&<p className="collection-body muted" role="status">{search.trim()?"No recordings match your search.":"No recordings yet. Upload an MCAP to get started."}</p>}
      <div className="recording-pagination"><span>{recordings.length?`${current*pageSize+1}–${Math.min((current+1)*pageSize,recordings.length)} of ${recordings.length}`:"0 recordings"}</span><div className="button-row"><button className="button secondary small" aria-label="Previous recordings page" disabled={current===0} onClick={()=>setPage(current-1)}><ChevronLeft size={16}/>Previous</button><span>Page {current+1} of {lastPage+1}</span><button className="button secondary small" aria-label="Next recordings page" disabled={current>=lastPage} onClick={()=>setPage(current+1)}>Next<ChevronRight size={16}/></button></div></div>
    </section>
  </div>;
}
