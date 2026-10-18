import { Fragment, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, File, Search, Upload } from "lucide-react";
import type { WorkspaceController } from "../hooks/useWorkspace";
import type { RecordingSummary, RecordingTopicInfo } from "../types/models";
import { can } from "../utils/permissions";
import { filename } from "../utils/recordings";
import { useReviewTasks } from "../hooks/useReviewTasks";
import { RecordingActions } from "../components/mcaps/RecordingActions";
import { latestRecordingTask } from "../utils/reviews";
import { libraryDuration, libraryFileSize, libraryStatuses, libraryWorkflow, recordingFamilies } from "../utils/library";

export function McapsPage({ workspace }: { workspace: WorkspaceController; uploadButton?: ReactNode }) {
  const {user,project,busy,setError,search,setSearch,dateSort,setDateSort,setRemoveTarget,chooseRecording,openReview}=workspace;
  const [page,setPage]=useState(0),[pageSize,setPageSize]=useState(10),[dragging,setDragging]=useState(false),[dropzone,setDropzone]=useState(false),[filter,setFilter]=useState("");
  const [expanded,setExpanded]=useState<{id:string;section:"topics"|"datasets"}|null>(null);
  const reviewAccess=can(project,user,"annotation.view")||can(project,user,"review.view");
  const reviews=useReviewTasks(project?.uuid||"",setError,!!project&&reviewAccess);
  if(!project)return null;
  const manage=can(project,user,"project.manage"),label=can(project,user,"annotation.create")||can(project,user,"annotation.edit");
  const all=recordingFamilies(project.mcaps).map(group=>({...group,progress:libraryWorkflow(group.item,reviews.tasks)}));
  const recordings=all.filter(group=>(!filter||group.progress.status===filter)&&group.members.some(item=>`${filename(item.source_file)} ${(item.topic_details||[]).map(topic=>`${topic.topic} ${topic.schema_name} ${topic.modality}`).join(" ")} ${(item.dataset_usages||[]).map(usage=>usage.name).join(" ")}`.toLowerCase().includes(search.trim().toLowerCase())))
    .sort((a,b)=>(dateSort==="newest"?-1:1)*((Date.parse(a.item.created_at||"")||0)-(Date.parse(b.item.created_at||"")||0)));
  const lastPage=Math.max(0,Math.ceil(recordings.length/pageSize)-1),current=Math.min(page,lastPage),rows=recordings.slice(current*pageSize,(current+1)*pageSize);
  const uploadEnabled=manage&&!busy&&workspace.connected===true;
  const totalTopics=all.reduce((sum,{item})=>sum+(item.topic_count??item.summary?.topic_count??0),0),seconds=all.reduce((sum,{item})=>sum+(item.duration_seconds??item.summary?.duration_seconds??0),0);
  const expand=(id:string,section:"topics"|"datasets")=>setExpanded(value=>value?.id===id&&value.section===section?null:{id,section});
  const openTopic=async(item:RecordingSummary,topic:RecordingTopicInfo)=>{
    await chooseRecording(item.uuid,undefined,recording=>{
      const selected=recording.topics.find(value=>String(value.channel_id)===topic.channel_id);
      if(!selected?.frame_count)return;
      workspace.setChannel(topic.channel_id);workspace.setPage(selected.kind==="image"?"labeling":"structured");
    });
  };
  return <div className="feature-stack recordings-page" data-file-drop
    onDragEnter={event=>{if(event.dataTransfer.types.includes("Files")){event.preventDefault();if(uploadEnabled)setDragging(true);}}}
    onDragOver={event=>{if(event.dataTransfer.types.includes("Files")){event.preventDefault();event.dataTransfer.dropEffect=uploadEnabled?"copy":"none";}}}
    onDragLeave={event=>{if(!(event.relatedTarget instanceof Node)||!event.currentTarget.contains(event.relatedTarget))setDragging(false);}}
    onDrop={event=>{event.preventDefault();setDragging(false);if(uploadEnabled)void workspace.upload(Array.from(event.dataTransfer.files));}}>
    <div className="page-heading library-heading"><div><h1>MCAP Library</h1><p>Manage project recordings and their annotations.</p><span className="muted">{all.length} recordings · {totalTopics.toLocaleString()} topics · {seconds.toLocaleString(undefined,{maximumFractionDigits:1})} seconds of recorded data</span></div>{manage&&<div className="button-row"><button className="button primary" disabled={!uploadEnabled} onClick={()=>workspace.uploadRef.current?.click()}><Upload size={16}/>Import MCAPs</button><button className="text-button" aria-expanded={dropzone} onClick={()=>setDropzone(value=>!value)}>{dropzone?"Hide dropzone":"Show dropzone"}</button></div>}</div>
    {(dropzone||dragging)&&manage&&<section aria-label="Upload MCAP recordings"><button type="button" className={`library-dropzone ${dragging?"is-dragging":""}`} disabled={!uploadEnabled} onClick={()=>workspace.uploadRef.current?.click()}><Upload size={20}/><span>Click to choose MCAP files or drop them here.</span></button></section>}
    <div className="library-stats" aria-label="Recording overview"><div><strong>{all.length}</strong><span>Recordings</span></div><div><strong>{totalTopics.toLocaleString()}</strong><span>Topics</span></div><div><strong>{all.filter(group=>group.progress.status==="Approved").length}</strong><span>Approved</span></div></div>
    <section className="panel library-panel" aria-label="MCAP recordings"><div className="collection-body recording-table-controls"><label className="recording-search"><Search size={17}/><input type="search" aria-label="Search included MCAPs" placeholder="Search recordings, topics, or datasets…" value={search} onChange={event=>{setSearch(event.target.value);setPage(0);}}/></label><div className="button-row"><label>Status<select aria-label="Recording workflow filter" value={filter} onChange={event=>{setFilter(event.target.value);setPage(0);}}><option value="">All recordings</option>{libraryStatuses.map(status=><option key={status}>{status}</option>)}</select></label><label>Sort<select aria-label="Sort MCAPs by import date" value={dateSort} onChange={event=>{setDateSort(event.target.value);setPage(0);}}><option value="newest">Newest first</option><option value="oldest">Oldest first</option></select></label>{all.length>10&&<label>Rows<select aria-label="Recordings per page" value={pageSize} onChange={event=>{setPageSize(Number(event.target.value));setPage(0);}}>{[10,20,30,40,50].map(size=><option key={size}>{size}</option>)}</select></label>}</div></div>
      <div className="table-wrap"><table className="recordings-table"><thead><tr><th>Recording</th><th>Topics</th><th>Duration</th><th>File size</th><th>Workflow</th><th>Datasets</th><th>Action</th></tr></thead><tbody>
        {rows.map(({item,members,progress})=>{
          const task=latestRecordingTask(reviews.tasks,item.uuid),open=expanded?.id===item.uuid;
          const modalities=[...new Set((item.topic_details||[]).map(topic=>topic.modality))];
          const usages=members.flatMap(member=>member.dataset_usages||[]),datasets=[...new Map(usages.map(usage=>[usage.dataset_id,usage])).values()],usageAvailable=members.every(member=>member.dataset_usage_available!==false);

          return <Fragment key={item.uuid}>
            <tr className="recording-table-row" tabIndex={0} aria-expanded={open} aria-label={`${filename(item.source_file)}, ${progress.status}`} onClick={event=>{if(!(event.target as Element).closest("button,a,summary,details,input,select"))expand(item.uuid,"topics");}} onKeyDown={event=>{if(event.target===event.currentTarget&&["Enter"," "].includes(event.key)){event.preventDefault();expand(item.uuid,"topics");}}}>
              <td><button className="recording-name-button" aria-label={`View topics for ${filename(item.source_file)}`} onClick={()=>expand(item.uuid,"topics")}>{open?<ChevronDown size={16}/>:<ChevronRight size={16}/>}<File size={18}/><span><strong>{filename(item.source_file)}</strong>{item.created_at&&<small>{new Date(item.created_at).toLocaleDateString(undefined,{month:"short",day:"numeric",year:"numeric"})}</small>}</span></button><div className="recording-modalities">{modalities.slice(0,3).map(modality=><span className="pill" key={modality}>{modality}</span>)}{modalities.length>3&&<span className="pill" title={modalities.slice(3).join(", ")}>+{modalities.length-3} more</span>}{!modalities.length&&<span className="muted">Inspect modalities</span>}</div></td>
              <td>{item.topic_count??item.summary?.topic_count??"Unavailable"}</td><td>{libraryDuration(item.duration_seconds??item.summary?.duration_seconds)}</td><td>{libraryFileSize(item.file_size_bytes)}</td>
              <td>{task?.current_revision?<button className={`pill mcap-review-status ${progress.status==="Approved"?"green":progress.status==="Changes requested"?"amber":"blue"}`} aria-label={`Review status for ${filename(item.source_file)}: ${progress.status}`} disabled={!!busy} onClick={()=>void openReview({taskId:task.id,view:progress.status==="Changes requested"?"comments":"review"})}>{progress.status}</button>:<span className="pill">{progress.status}</span>}<small className="table-secondary">{progress.labeled.toLocaleString()} labeled{progress.total>0?` · ${progress.approved.toLocaleString()}/${progress.total.toLocaleString()} approved`:""}</small></td>
              <td><button className="text-button" onClick={()=>expand(item.uuid,"datasets")}>{usageAvailable?`Used in ${datasets.length} ${datasets.length===1?"dataset":"datasets"}`:"Unavailable"}</button></td>
              <td><div className="button-row"><button className="button secondary small" aria-label={`Open recording ${filename(item.source_file)}`} disabled={!!busy} onClick={()=>void chooseRecording(item.uuid)}>Open</button><RecordingActions name={filename(item.source_file)} actions={[
                  {label:"Open",disabled:!!busy,run:()=>void chooseRecording(item.uuid)},
                  {label:"View topics",run:()=>setExpanded({id:item.uuid,section:"topics"})},
                  ...(label?[{label:"Label",disabled:!!busy,run:()=>void workspace.startLabeling(item.uuid,[item.uuid])}]:[]),
                  {label:"View datasets",run:()=>setExpanded({id:item.uuid,section:"datasets"})},
                  ...(manage?[{label:"Delete",danger:true,disabled:!!busy,run:()=>{setError("");setRemoveTarget(item);}}]:[]),
                ]}/></div></td>
            </tr>
            {open&&<tr className="recording-expanded"><td colSpan={7}><div className="recording-expanded-body"><div className="button-row"><button className={`button ${expanded.section==="topics"?"primary":"secondary"} small`} onClick={()=>setExpanded({id:item.uuid,section:"topics"})}>Topics</button><button className={`button ${expanded.section==="datasets"?"primary":"secondary"} small`} onClick={()=>setExpanded({id:item.uuid,section:"datasets"})}>Datasets</button></div>
              {expanded.section==="topics"?<><p className="field-help">Modalities are detected from topic and schema identities. Approval covers required labeling topics and ranges, independently of dataset reuse.</p>{item.topic_details?.length?<table className="library-topic-details"><thead><tr><th>Topic</th><th>Type</th><th>Schema / encoding</th><th>Action</th></tr></thead><tbody>{item.topic_details.map(topic=><tr key={topic.channel_id}><td>{topic.topic}</td><td>{topic.modality==="Video"?"Video":["Structured","Metadata"].includes(topic.modality)?"Structured":"Sensor"}</td><td>{topic.schema_name||topic.encoding}</td><td><button className="button secondary small" disabled={!!busy} onClick={()=>void openTopic(item,topic)}>{label?"Label topic":"View topic"}</button></td></tr>)}</tbody></table>:<button className="button secondary small" disabled={!!busy} onClick={()=>void chooseRecording(item.uuid)}>Open recording to inspect topics</button>}</>:
                <><p className="field-help">Published datasets in this project. Draft selections are not counted.</p>{!usageAvailable?<p role="status">Published references could not be loaded. Refresh to try again.</p>:datasets.length?datasets.map(dataset=><article className="library-dataset-reference" key={dataset.dataset_id}><strong>{dataset.name}</strong><span>{[...new Set(usages.filter(usage=>usage.dataset_id===dataset.dataset_id).map(usage=>`Version ${usage.version} · revision ${usage.revision}`))].join("; ")}</span></article>):<p className="muted">No published dataset versions reference this recording yet.</p>}</>}
            </div></td></tr>}
          </Fragment>;
        })}
      </tbody></table></div>
      {!rows.length&&<p className="collection-body muted" role="status">{search.trim()||filter?"No recordings match your filters.":"Import an MCAP to start building your recording inventory."}</p>}
      {recordings.length>pageSize&&<div className="recording-pagination"><span>{current*pageSize+1}–{Math.min((current+1)*pageSize,recordings.length)} of {recordings.length}</span><div className="button-row"><button className="button secondary small" aria-label="Previous recordings page" disabled={current===0} onClick={()=>setPage(current-1)}>Previous</button><span>Page {current+1} of {lastPage+1}</span><button className="button secondary small" aria-label="Next recordings page" disabled={current>=lastPage} onClick={()=>setPage(current+1)}>Next</button></div></div>}
    </section>
  </div>;
}
